/*
 * STC8H1K08 八路 PWM 控制器固件 —— 主循环与命令分发
 *
 * 通信协议：docs/design.md
 */
#include "fw_hal.h"
#include "protocol.h"
#include "pwm_ctrl.h"
#include "uart_io.h"

#define FW_PROTO_VER 2
#define FW_MAJOR    1
#define FW_MINOR    1

/* 解析器与批量缓冲放 XDATA，节省仅 128B 的 DATA 区（SDCC 局部变量静态叠放） */
static __xdata proto_parser_t parser;
static __xdata uint8_t resp_buf[31];   /* 应答最长 6 + 25 */
static __xdata uint8_t qdata[25];      /* 查询应答数据区 */
static __xdata uint16_t duty_buf[PWM_CH_COUNT];
static __xdata pwm_actual_t qactual;

static uint16_t rd_u16(const uint8_t *p)
{
    return ((uint16_t)p[0] << 8) | p[1]; /* 大端序 */
}

static void wr_u16(uint8_t *p, uint16_t v)
{
    p[0] = (uint8_t)(v >> 8);
    p[1] = (uint8_t)v;
}

static void send_resp(uint8_t cmd, uint8_t status, const uint8_t *data, uint8_t dlen)
{
    uint8_t n = proto_build_resp(resp_buf, cmd, status, data, dlen);
    uart_io_write(resp_buf, n);
}

static void dispatch(void)
{
    uint8_t cmd = parser.cmd;
    uint8_t len = parser.len;

    switch (cmd)
    {
    case PROTO_CMD_SET_ALL: /* [fA u16][fB u16][8 x 占空比 u16] */
    {
        uint8_t i;

        if (len != 20) { send_resp(cmd, PROTO_ST_LEN, 0, 0); break; }
        if (pwm_estop_active()) { send_resp(cmd, PROTO_ST_ESTOP, 0, 0); break; }
        for (i = 0; i < PWM_CH_COUNT; i++)
            duty_buf[i] = rd_u16(&parser.payload[4 + i * 2]);
        if (pwm_set_all(rd_u16(&parser.payload[0]), rd_u16(&parser.payload[2]), duty_buf) != 0)
        {
            send_resp(cmd, PROTO_ST_PARAM, 0, 0);
            break;
        }
        P13 = 1;
        send_resp(cmd, PROTO_ST_OK, 0, 0);
        break;
    }

    case PROTO_CMD_SET_DUTY: /* [通道号][占空比 u16] */
    {
        if (len != 3) { send_resp(cmd, PROTO_ST_LEN, 0, 0); break; }
        if (pwm_estop_active()) { send_resp(cmd, PROTO_ST_ESTOP, 0, 0); break; }
        if (pwm_set_duty(parser.payload[0], rd_u16(&parser.payload[1])) != 0)
        {
            send_resp(cmd, PROTO_ST_PARAM, 0, 0);
            break;
        }
        P13 = 1;
        send_resp(cmd, PROTO_ST_OK, 0, 0);
        break;
    }

    case PROTO_CMD_SET_DUTIES: /* [8 x 占空比 u16] */
    {
        uint8_t i;

        if (len != 16) { send_resp(cmd, PROTO_ST_LEN, 0, 0); break; }
        if (pwm_estop_active()) { send_resp(cmd, PROTO_ST_ESTOP, 0, 0); break; }
        for (i = 0; i < PWM_CH_COUNT; i++)
            duty_buf[i] = rd_u16(&parser.payload[i * 2]);
        if (pwm_set_duties(duty_buf) != 0)
        {
            send_resp(cmd, PROTO_ST_PARAM, 0, 0);
            break;
        }
        P13 = 1;
        send_resp(cmd, PROTO_ST_OK, 0, 0);
        break;
    }

    case PROTO_CMD_ESTOP:
    {
        if (len != 0) { send_resp(cmd, PROTO_ST_LEN, 0, 0); break; }
        pwm_estop();
        P13 = 0;
        send_resp(cmd, PROTO_ST_OK, 0, 0);
        break;
    }

    case PROTO_CMD_RESUME:
    {
        if (len != 0) { send_resp(cmd, PROTO_ST_LEN, 0, 0); break; }
        pwm_resume();
        P13 = pwm_configured() ? 1 : 0;
        send_resp(cmd, PROTO_ST_OK, 0, 0);
        break;
    }

    case PROTO_CMD_QUERY: /* 回读实际生效值：flags + fA/fB 实际频率 + NA/NB 分辨率 + 8 x 实际占空比 */
    {
        uint8_t i;

        if (len != 0) { send_resp(cmd, PROTO_ST_LEN, 0, 0); break; }
        pwm_get_actual(&qactual);
        qdata[0] = (uint8_t)((pwm_estop_active() ? 0x01 : 0x00) |
                             (pwm_configured() ? 0x02 : 0x00) |
                             (qactual.freq_a ? 0x04 : 0x00) |
                             (qactual.freq_b ? 0x08 : 0x00));
        wr_u16(&qdata[1], qactual.freq_a);
        wr_u16(&qdata[3], qactual.freq_b);
        wr_u16(&qdata[5], qactual.n_a);
        wr_u16(&qdata[7], qactual.n_b);
        for (i = 0; i < PWM_CH_COUNT; i++)
            wr_u16(&qdata[9 + i * 2], qactual.duty[i]);
        send_resp(cmd, PROTO_ST_OK, qdata, 25);
        break;
    }

    case PROTO_CMD_VERSION:
    {
        if (len != 0) { send_resp(cmd, PROTO_ST_LEN, 0, 0); break; }
        qdata[0] = FW_PROTO_VER;
        qdata[1] = FW_MAJOR;
        qdata[2] = FW_MINOR;
        send_resp(cmd, PROTO_ST_OK, qdata, 3);
        break;
    }

    default:
        send_resp(cmd, PROTO_ST_PARAM, 0, 0);
        break;
    }
}

void main(void)
{
    /*
     * 时钟初始化：SYSCLK = MCLK（IRC 频率在 ISP 下载时设定）。
     * CLKDIV 复位值为 4（SYSCLK = MCLK/4，数据手册 §6.2.4），此处置 0 取全速。
     * 故意不调用 SYS_SetClock()：它会重写 IRC 微调寄存器
     * （IRCBAND/VRTRIM/IRTRIM/LIRTRIM），数据手册 §6.2 明确警告不调整频率时
     * 不能随意修改这些寄存器，否则运行频率会变。
     */
    SFRX_ON();
    CLKDIV = 0x00;
    SFRX_OFF();

    /* P1.1：初始化完成指示；P1.3：PWM 已配置指示。
     * 复位后 P1.x 为高阻输入（数据手册 §2.1.2），须显式配置为推挽输出。 */
    GPIO_P1_SetMode(GPIO_Pin_1 | GPIO_Pin_3, GPIO_Mode_Output_PP);
    P13 = 0;

    pwm_init();
    uart_io_init();
    tick_init();
    proto_init(&parser);

    P11 = 1; /* 初始化完成 */

    while (1)
    {
        uint8_t b;
        while (uart_io_read(&b))
        {
            proto_event_t ev = proto_feed_byte(&parser, b, (uint16_t)tick_ms);
            switch (ev)
            {
            case PROTO_EVT_FRAME:
                dispatch();
                break;
            case PROTO_EVT_ERR_CRC:
                send_resp(parser.cmd, PROTO_ST_CRC, 0, 0);
                break;
            case PROTO_EVT_ERR_LEN:
                send_resp(parser.cmd, PROTO_ST_LEN, 0, 0);
                break;
            default:
                break;
            }
        }
    }
}
