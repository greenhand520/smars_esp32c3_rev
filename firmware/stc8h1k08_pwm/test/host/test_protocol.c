/*
 * protocol.c 主机端单元测试（gcc 编译，不依赖目标硬件）
 *
 * 覆盖：CRC8 测试向量、帧解析、噪声重同步、坏 CRC、非法长度、
 *       帧间超时、应答帧构造回读。
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "../../src/protocol.h"

static int failures = 0;

#define CHECK(cond) do { \
    if (!(cond)) { \
        printf("FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); \
        failures++; \
    } \
} while (0)

/* 独立的按位参考实现，与协议实现交叉验证 */
static uint8_t ref_crc8(const uint8_t *data, uint8_t len)
{
    uint8_t crc = 0, i, b;
    for (i = 0; i < len; i++)
    {
        crc ^= data[i];
        for (b = 0; b < 8; b++)
            crc = (crc & 0x80) ? (uint8_t)((crc << 1) ^ 0x07) : (uint8_t)(crc << 1);
    }
    return crc;
}

static proto_event_t feed(proto_parser_t *p, const uint8_t *bytes,
                          uint8_t n, uint16_t *ms)
{
    proto_event_t ev = PROTO_EVT_NONE, last = PROTO_EVT_NONE;
    uint8_t i;
    for (i = 0; i < n; i++)
    {
        ev = proto_feed_byte(p, bytes[i], *ms);
        *ms += 1; /* 字节间隔 1ms，远小于 5ms 超时 */
        if (ev != PROTO_EVT_NONE)
            last = ev;
    }
    return last;
}

static void test_crc_vectors(void)
{
    const uint8_t v1[] = {0x01, 0x02, 0x03, 0x04};
    const uint8_t v2[] = {0x04, 0x00};
    const uint8_t v3[] = {0x02, 0x03, 0x00, 0x13, 0x88};

    CHECK(proto_crc8(v1, sizeof v1) == 0xE3);
    CHECK(proto_crc8(v2, sizeof v2) == 0x54);
    CHECK(proto_crc8(v3, sizeof v3) == 0x27);

    /* 与参考实现一致 */
    {
        uint8_t buf[64];
        int i;
        for (i = 0; i < 64; i++)
            buf[i] = (uint8_t)(i * 37 + 11);
        CHECK(proto_crc8(buf, 64) == ref_crc8(buf, 64));
        CHECK(proto_crc8(buf, 1) == ref_crc8(buf, 1));
        CHECK(proto_crc8(buf, 0) == 0x00);
    }
}

static void test_valid_frames(void)
{
    proto_parser_t p;
    uint16_t ms = 0;

    /* 急停帧：aa 55 04 00 crc */
    {
        uint8_t frame[5];
        frame[0] = 0xAA; frame[1] = 0x55; frame[2] = 0x04; frame[3] = 0x00;
        frame[4] = proto_crc8(&frame[2], 2);
        proto_init(&p);
        CHECK(feed(&p, frame, 5, &ms) == PROTO_EVT_FRAME);
        CHECK(p.cmd == PROTO_CMD_ESTOP);
        CHECK(p.len == 0);
    }

    /* 单通道帧：ch0 duty 5000（0x1388） */
    {
        uint8_t frame[8];
        frame[0] = 0xAA; frame[1] = 0x55; frame[2] = 0x02; frame[3] = 0x03;
        frame[4] = 0x00; frame[5] = 0x13; frame[6] = 0x88;
        frame[7] = proto_crc8(&frame[2], 5);
        proto_init(&p);
        CHECK(feed(&p, frame, 8, &ms) == PROTO_EVT_FRAME);
        CHECK(p.cmd == PROTO_CMD_SET_DUTY);
        CHECK(p.len == 3);
        CHECK(p.payload[0] == 0x00 && p.payload[1] == 0x13 && p.payload[2] == 0x88);
    }

    /* 32 字节满载载荷合法 */
    {
        uint8_t frame[2 + 1 + 1 + 32 + 1];
        uint8_t i;
        frame[0] = 0xAA; frame[1] = 0x55; frame[2] = 0x01; frame[3] = 32;
        for (i = 0; i < 32; i++)
            frame[4 + i] = (uint8_t)(i + 1);
        frame[36] = proto_crc8(&frame[2], 34);
        proto_init(&p);
        CHECK(feed(&p, frame, 37, &ms) == PROTO_EVT_FRAME);
        CHECK(p.len == 32 && p.payload[31] == 32);
    }
}

static void test_resync(void)
{
    proto_parser_t p;
    uint16_t ms = 0;

    /* 帧前噪声 */
    {
        const uint8_t noise[] = {0x00, 0xFF, 0x55, 0x13};
        uint8_t frame[5] = {0xAA, 0x55, 0x04, 0x00, 0x54};
        proto_init(&p);
        CHECK(feed(&p, noise, 4, &ms) == PROTO_EVT_NONE);
        CHECK(feed(&p, frame, 5, &ms) == PROTO_EVT_FRAME);
    }

    /* AA AA 55 连帧头容忍 */
    {
        const uint8_t frame[] = {0xAA, 0xAA, 0x55, 0x04, 0x00, 0x54};
        proto_init(&p);
        CHECK(feed(&p, frame, 6, &ms) == PROTO_EVT_FRAME);
        CHECK(p.cmd == PROTO_CMD_ESTOP);
    }

    /* 坏 CRC：报错后可继续解析下一帧 */
    {
        uint8_t bad[5] = {0xAA, 0x55, 0x04, 0x00, 0x00};
        uint8_t good[5] = {0xAA, 0x55, 0x04, 0x00, 0x54};
        proto_init(&p);
        CHECK(feed(&p, bad, 5, &ms) == PROTO_EVT_ERR_CRC);
        CHECK(feed(&p, good, 5, &ms) == PROTO_EVT_FRAME);
    }

    /* 坏 CRC 帧之后噪声 + 下一帧正常 */
    {
        uint8_t bad[8] = {0xAA, 0x55, 0x02, 0x03, 0x00, 0x13, 0x88, 0x00};
        uint8_t good[5] = {0xAA, 0x55, 0x04, 0x00, 0x54};
        proto_init(&p);
        CHECK(feed(&p, bad, 8, &ms) == PROTO_EVT_ERR_CRC);
        CHECK(feed(&p, good, 5, &ms) == PROTO_EVT_FRAME);
    }
}

static void test_bad_len(void)
{
    proto_parser_t p;
    uint16_t ms = 0;
    uint8_t hdr[4] = {0xAA, 0x55, 0x01, 33}; /* len > 32 */
    uint8_t good[5] = {0xAA, 0x55, 0x04, 0x00, 0x54};

    proto_init(&p);
    CHECK(feed(&p, hdr, 4, &ms) == PROTO_EVT_ERR_LEN);
    CHECK(feed(&p, good, 5, &ms) == PROTO_EVT_FRAME);
}

static void test_timeout(void)
{
    proto_parser_t p;
    const uint8_t partial[3] = {0xAA, 0x55, 0x01};
    const uint8_t frame[5] = {0xAA, 0x55, 0x04, 0x00, 0x54};
    uint8_t i;

    /* 半帧后间隙 6ms（>5ms）：残留字节被丢弃，新帧从帧头重新解析 */
    proto_init(&p);
    for (i = 0; i < 3; i++)
        CHECK(proto_feed_byte(&p, partial[i], (uint16_t)(100 + i)) == PROTO_EVT_NONE);
    for (i = 0; i < 5; i++)
        CHECK(proto_feed_byte(&p, frame[i], (uint16_t)(108 + i)) ==
              ((i == 4) ? PROTO_EVT_FRAME : PROTO_EVT_NONE));
    CHECK(p.cmd == PROTO_CMD_ESTOP);

    /* 同一帧内分包，帧内间隙恰 5ms（边界值）：不算超时，整帧正常拼回 */
    proto_init(&p);
    for (i = 0; i < 3; i++)
        CHECK(proto_feed_byte(&p, partial[i], (uint16_t)(100 + i)) == PROTO_EVT_NONE);
    /* 该帧 = AA 55 01 <len=0> <crc>，剩余 2 字节间隔 5ms 到达 */
    {
        const uint8_t rest[2] = {0x00, proto_crc8((const uint8_t *)"\x01\x00", 2)};
        for (i = 0; i < 2; i++)
            CHECK(proto_feed_byte(&p, rest[i], (uint16_t)(107 + i)) ==
                  ((i == 1) ? PROTO_EVT_FRAME : PROTO_EVT_NONE));
        CHECK(p.cmd == PROTO_CMD_SET_ALL && p.len == 0);
    }
}

static void test_build_resp(void)
{
    proto_parser_t p;
    uint16_t ms = 0;
    uint8_t buf[32];
    uint8_t data[21];
    uint8_t i, n;

    for (i = 0; i < 21; i++)
        data[i] = (uint8_t)(0xF0 + i);

    n = proto_build_resp(buf, PROTO_CMD_QUERY, PROTO_ST_OK, data, 21);
    CHECK(n == 27);
    CHECK(buf[0] == 0xAA && buf[1] == 0x55);
    CHECK(buf[2] == (PROTO_CMD_QUERY | PROTO_RESP_FLAG));
    CHECK(buf[3] == 22); /* 状态 + 21 数据 */
    CHECK(buf[4] == PROTO_ST_OK);
    CHECK(buf[5] == 0xF0 && buf[25] == (uint8_t)(0xF0 + 20));
    CHECK(buf[26] == proto_crc8(&buf[2], 24));

    /* 应答帧能被同一解析器正确解析（回环） */
    proto_init(&p);
    CHECK(feed(&p, buf, n, &ms) == PROTO_EVT_FRAME);
    CHECK(p.cmd == (PROTO_CMD_QUERY | PROTO_RESP_FLAG));
    CHECK(p.len == 22);
    CHECK(p.payload[0] == PROTO_ST_OK);

    /* 无数据应答 */
    n = proto_build_resp(buf, PROTO_CMD_ESTOP, PROTO_ST_OK, NULL, 0);
    CHECK(n == 6);
    proto_init(&p);
    CHECK(feed(&p, buf, n, &ms) == PROTO_EVT_FRAME);
    CHECK(p.len == 1 && p.payload[0] == PROTO_ST_OK);
}

int main(void)
{
    test_crc_vectors();
    test_valid_frames();
    test_resync();
    test_bad_len();
    test_timeout();
    test_build_resp();

    if (failures)
    {
        printf("%d check(s) FAILED\n", failures);
        return 1;
    }
    printf("all protocol tests passed\n");
    return 0;
}
