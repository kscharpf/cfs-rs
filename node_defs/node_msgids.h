#ifndef NODE_MSGIDS_H
#define NODE_MSGIDS_H

/*
 * Internal Software Bus Message IDs for the scenario (link-lab design §4.4).
 *
 * cFE platform and global reserved ranges (from C3):
 *   Platform Telemetry: 0x0800 .. 0x085F
 *   Global Telemetry:   0x0860 .. 0x087F
 *   Platform Command:   0x1800 .. 0x185F
 *   Global Command:     0x1860 .. 0x187F
 *
 * To avoid any collision with cFE core or platform messages, we allocate:
 *   Commands / Wakes (type = 1): base 0x1900
 *   Telemetry / Data (type = 0): base 0x0900
 */

#define NODE_CMD_MID_BASE 0x1900
#define NODE_TLM_MID_BASE 0x0900

/* Commands / Turns */
#define NODE_START_MID           (NODE_CMD_MID_BASE + 0x00) /* step_lab -> all: link level of case */
#define NODE_WAKE_RX_MID         (NODE_CMD_MID_BASE + 0x01) /* step_lab -> link_lab: process delivered unit */
#define NODE_WAKE_TX_MID         (NODE_CMD_MID_BASE + 0x02) /* step_lab -> link_lab: drain pipe, frame of slot */
#define NODE_WAKE_APPS_MID       (NODE_CMD_MID_BASE + 0x03) /* step_lab -> scn_lab: scenario steps 2-5 */
#define NODE_CMD_PACKET_MID      (NODE_CMD_MID_BASE + 0x04) /* link_lab -> scn_lab: one command packet */
#define NODE_CMD_PACKET_DONE_MID (NODE_CMD_MID_BASE + 0x05) /* scn_lab -> link_lab: return turn */

/* Telemetry / Data */
#define NODE_TLM_PACKET_MID      (NODE_TLM_MID_BASE + 0x00) /* scn_lab -> link_lab: one telemetry packet */
#define NODE_RECORD_MID          (NODE_TLM_MID_BASE + 0x01) /* scn_lab -> link_lab -> step_lab: app_event */
#define NODE_UNIT_MID            (NODE_TLM_MID_BASE + 0x02) /* link_lab -> step_lab: one link unit */
#define NODE_DONE_MID            (NODE_TLM_MID_BASE + 0x03) /* app -> step_lab: next deadline, ends turn */

/*
 * Build-time check: verify that no internal node message ID falls within
 * cFE's reserved platform or global ranges.
 */
#define NODE_IS_RESERVED_CFE_MID(mid) \
    (((mid) >= 0x0800 && (mid) <= 0x087F) || \
     ((mid) >= 0x1800 && (mid) <= 0x187F))

#define NODE_CHECK_MID(mid) \
    _Static_assert(!NODE_IS_RESERVED_CFE_MID(mid), "Message ID " #mid " collides with cFE reserved ranges")

NODE_CHECK_MID(NODE_START_MID);
NODE_CHECK_MID(NODE_WAKE_RX_MID);
NODE_CHECK_MID(NODE_WAKE_TX_MID);
NODE_CHECK_MID(NODE_WAKE_APPS_MID);
NODE_CHECK_MID(NODE_CMD_PACKET_MID);
NODE_CHECK_MID(NODE_CMD_PACKET_DONE_MID);
NODE_CHECK_MID(NODE_TLM_PACKET_MID);
NODE_CHECK_MID(NODE_RECORD_MID);
NODE_CHECK_MID(NODE_UNIT_MID);
NODE_CHECK_MID(NODE_DONE_MID);

#endif /* NODE_MSGIDS_H */
