#include "cfe_tbl_filedef.h"
#include "sch_lab_tbl.h"
#include "cfe_sb_api_typedefs.h"
#include "cfe_msgids.h"
#include "node_msgids.h"

SCH_LAB_ScheduleTable_t Schedule = {
    .TickRate = 10,
    .Config   = {
        {CFE_SB_MSGID_WRAP_VALUE(NODE_WAKE_TX_MID), 1, 0},
        {CFE_SB_MSGID_WRAP_VALUE(CFE_ES_SEND_HK_MID), 20, 0},
        {CFE_SB_MSGID_WRAP_VALUE(CFE_EVS_SEND_HK_MID), 21, 0},
        {CFE_SB_MSGID_WRAP_VALUE(CFE_SB_SEND_HK_MID), 22, 0},
        {CFE_SB_MSGID_WRAP_VALUE(CFE_TIME_SEND_HK_MID), 23, 0},
        {CFE_SB_MSGID_WRAP_VALUE(CFE_TBL_SEND_HK_MID), 24, 0},
        {CFE_SB_MSGID_RESERVED, 0, 0},
    }
};

CFE_TBL_FILEDEF(Schedule, SCH_LAB.Schedule, Schedule Lab MsgID Table, sch_lab_table.tbl)
