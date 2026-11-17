#include "cfe_tbl_filedef.h"
#include "common_types.h"

typedef struct
{
    uint16 apid;
    uint8  vcid;
    uint8  reserved;
} LINK_LAB_ApidRoute_t;

typedef struct
{
    uint64               frame_slot_us;
    uint64               vc_realtime_deadline_us;
    uint64               vc_payload_deadline_us;
    uint16               spacecraft_id;
    uint16               tm_frame_length;
    uint16               tc_max_frame_length;
    uint8                tm_fecf;
    uint8                tc_fecf;
    uint8                cop1_w;
    uint8                cop1_status_field;
    uint8                farm_initial_vr;
    uint8                route_count;
    uint8                reserved[4];
    LINK_LAB_ApidRoute_t routes[8];
    uint16               rx_port;
    uint16               tx_port;
    uint8                ground_ipv4[4];
    uint8                mode;
    uint8                default_vc;
    uint8                reserved2[6];
} LINK_LAB_CfgTbl_t;

CompileTimeAssert(sizeof(LINK_LAB_ApidRoute_t) == 4, LinkLabApidRouteSize);
CompileTimeAssert(sizeof(LINK_LAB_CfgTbl_t) == 88, LinkLabCfgTblSize);

LINK_LAB_CfgTbl_t LINK_LAB_CfgTbl = {
    125000ULL,  /* frame_slot_us */
    250000ULL,  /* vc_realtime_deadline_us */
    1000000ULL, /* vc_payload_deadline_us */
    341U,       /* spacecraft_id */
    223U,       /* tm_frame_length */
    128U,       /* tc_max_frame_length */
    1U,         /* tm_fecf */
    1U,         /* tc_fecf */
    8U,         /* cop1_w */
    0U,         /* cop1_status_field */
    0U,         /* farm_initial_vr */
    4U,         /* route_count */
    {0U, 0U, 0U, 0U},
    {
        {16U, 0U, 0U}, /* APID 16 (Housekeeping) -> VC 0 (Realtime) */
        {17U, 0U, 0U}, /* APID 17 (Events)       -> VC 0 (Realtime) */
        {32U, 1U, 0U}, /* APID 32 (Payload)      -> VC 1 (Payload) */
        {33U, 1U, 0U}, /* APID 33 (Dump)         -> VC 1 (Payload) */
        {0U, 0U, 0U},
        {0U, 0U, 0U},
        {0U, 0U, 0U},
        {0U, 0U, 0U},
    },
    1234U,               /* rx_port */
    2234U,               /* tx_port */
    {127U, 0U, 0U, 1U},  /* ground_ipv4 */
    0U,                  /* mode (0 = Lockstep) */
    0xFFU,               /* default_vc (0xFF = None) */
    {0U, 0U, 0U, 0U, 0U, 0U},
};

CFE_TBL_FILEDEF(LINK_LAB_CfgTbl, LINK_LAB.LinkCfg, link_lab LinkConfig table, link_lab_cfg.tbl)
