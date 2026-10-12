/*
 * 协议 v2：帧解析、CRC8、应答帧构造。
 * 纯 C99（无硬件依赖），可在主机端单元测试，见 test/host/。
 *
 * 帧（请求）：  AA 55 | cmd | len | payload[len] | crc8
 * 帧（应答）：  AA 55 | cmd|0x80 | len | status | data... | crc8
 * crc8 覆盖 cmd..payload（第 2..3+len 字节），多项式 0x07，初值 0x00。
 */
#ifndef PROTOCOL_H
#define PROTOCOL_H

#include <stdint.h>

#define PROTO_HDR1        0xAAu
#define PROTO_HDR2        0x55u
#define PROTO_RESP_FLAG   0x80u
#define PROTO_MAX_PAYLOAD 32u
#define PROTO_TIMEOUT_MS  5u

/* 命令 */
#define PROTO_CMD_SET_ALL     0x01u /* [fA u16][fB u16][8 x 占空比 u16] */
#define PROTO_CMD_SET_DUTY    0x02u /* [通道号][占空比 u16] */
#define PROTO_CMD_SET_DUTIES  0x03u /* [8 x 占空比 u16] */
#define PROTO_CMD_ESTOP       0x04u /* 空 */
#define PROTO_CMD_RESUME      0x05u /* 空 */
#define PROTO_CMD_QUERY       0x06u /* 空 */
#define PROTO_CMD_VERSION     0x07u /* 空 */

/* 状态码（应答帧数据区首字节） */
#define PROTO_ST_OK     0x00u
#define PROTO_ST_CRC    0x01u
#define PROTO_ST_LEN    0x02u
#define PROTO_ST_PARAM  0x03u
#define PROTO_ST_ESTOP  0x04u

typedef enum
{
    PROTO_EVT_NONE = 0,
    PROTO_EVT_FRAME,    /* 帧完整且 CRC 正确：cmd/len/payload 有效 */
    PROTO_EVT_ERR_CRC,  /* 帧被丢弃，cmd 为收到的命令字节 */
    PROTO_EVT_ERR_LEN,  /* 长度字段非法，帧被丢弃 */
} proto_event_t;

typedef struct
{
    uint8_t  state;
    uint8_t  cmd;
    uint8_t  len;
    uint8_t  idx;
    uint8_t  crc;
    uint16_t last_ms;
    uint8_t  payload[PROTO_MAX_PAYLOAD];
} proto_parser_t;

void proto_init(proto_parser_t *p);

/*
 * 喂入一个收到的字节。now_ms 为自由运行的毫秒计数；
 * 字节间隔 > PROTO_TIMEOUT_MS 会中止未完成的帧。
 */
proto_event_t proto_feed_byte(proto_parser_t *p, uint8_t b, uint16_t now_ms);

uint8_t proto_crc8(const uint8_t *data, uint8_t len);

/*
 * 构造应答帧写入 buf，buf 需容纳 6 + data_len 字节
 * （状态查询应答最长 27 字节）。返回帧总长度。
 */
uint8_t proto_build_resp(uint8_t *buf, uint8_t cmd, uint8_t status,
                         const uint8_t *data, uint8_t data_len);

#endif
