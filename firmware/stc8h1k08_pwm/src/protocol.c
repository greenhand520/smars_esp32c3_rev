#include "protocol.h"

enum
{
    S_HDR1 = 0,
    S_HDR2,
    S_CMD,
    S_LEN,
    S_DATA,
    S_CRC,
};

/* CRC-8/ATM：多项式 0x07，初值 0x00，不反转，无异或输出 */
uint8_t proto_crc8(const uint8_t *data, uint8_t len)
{
    uint8_t crc = 0x00;
    uint8_t i, b;

    for (i = 0; i < len; i++)
    {
        crc ^= data[i];
        for (b = 0; b < 8; b++)
        {
            if (crc & 0x80u)
                crc = (uint8_t)((crc << 1) ^ 0x07u);
            else
                crc = (uint8_t)(crc << 1);
        }
    }
    return crc;
}

static uint8_t crc_update(uint8_t crc, uint8_t byte)
{
    uint8_t b;
    crc ^= byte;
    for (b = 0; b < 8; b++)
    {
        if (crc & 0x80u)
            crc = (uint8_t)((crc << 1) ^ 0x07u);
        else
            crc = (uint8_t)(crc << 1);
    }
    return crc;
}

void proto_init(proto_parser_t *p)
{
    p->state = S_HDR1;
    p->cmd = 0;
    p->len = 0;
    p->idx = 0;
    p->crc = 0;
    p->last_ms = 0;
}

proto_event_t proto_feed_byte(proto_parser_t *p, uint8_t b, uint16_t now_ms)
{
    /* 字节间隔超时：丢弃未完成的帧，随后在帧头搜索态重新同步 */
    if (p->state != S_HDR1 &&
        (uint16_t)(now_ms - p->last_ms) > PROTO_TIMEOUT_MS)
    {
        p->state = S_HDR1;
    }
    p->last_ms = now_ms;

    switch (p->state)
    {
    case S_HDR1:
        if (b == PROTO_HDR1)
            p->state = S_HDR2;
        break;

    case S_HDR2:
        if (b == PROTO_HDR2)
            p->state = S_CMD;
        else if (b == PROTO_HDR1)
            p->state = S_HDR2;  /* 容忍 AA AA 55 */
        else
            p->state = S_HDR1;
        break;

    case S_CMD:
        p->cmd = b;
        p->crc = crc_update(0x00, b);
        p->state = S_LEN;
        break;

    case S_LEN:
        if (b > PROTO_MAX_PAYLOAD)
        {
            p->state = S_HDR1;
            return PROTO_EVT_ERR_LEN;
        }
        p->len = b;
        p->idx = 0;
        p->crc = crc_update(p->crc, b);
        p->state = (b == 0) ? S_CRC : S_DATA;
        break;

    case S_DATA:
        p->payload[p->idx++] = b;
        p->crc = crc_update(p->crc, b);
        if (p->idx >= p->len)
            p->state = S_CRC;
        break;

    case S_CRC:
        p->state = S_HDR1;
        if (b == p->crc)
            return PROTO_EVT_FRAME;
        return PROTO_EVT_ERR_CRC;

    default:
        p->state = S_HDR1;
        break;
    }
    return PROTO_EVT_NONE;
}

uint8_t proto_build_resp(uint8_t *buf, uint8_t cmd, uint8_t status,
                         const uint8_t *data, uint8_t data_len)
{
    uint8_t i;
    uint8_t len = (uint8_t)(data_len + 1); /* 状态 + 数据 */

    buf[0] = PROTO_HDR1;
    buf[1] = PROTO_HDR2;
    buf[2] = (uint8_t)(cmd | PROTO_RESP_FLAG);
    buf[3] = len;
    buf[4] = status;
    for (i = 0; i < data_len; i++)
        buf[5 + i] = data[i];
    buf[5 + data_len] = proto_crc8(&buf[2], (uint8_t)(len + 2));
    return (uint8_t)(6 + data_len);
}
