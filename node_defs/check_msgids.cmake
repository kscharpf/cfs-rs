# Build-time check: internal Software Bus message IDs against cFE reserved ranges (C3: 0x0800/0x1800, 0x0860/0x1860)

set(NODE_INTERNAL_MIDS
    0x1900 # NODE_START_MID
    0x1901 # NODE_WAKE_RX_MID
    0x1902 # NODE_WAKE_TX_MID
    0x1903 # NODE_WAKE_CMD_MID
    0x1904 # NODE_CMD_PACKET_MID
    0x1905 # NODE_CMD_PACKET_DONE_MID
    0x1906 # NODE_WAKE_HK_MID
    0x1907 # NODE_WAKE_PL_MID
    0x0900 # NODE_TLM_PACKET_MID
    0x0901 # NODE_RECORD_MID
    0x0902 # NODE_UNIT_MID
    0x0903 # NODE_DONE_MID
    0x0904 # NODE_SCN_STATUS_MID
)

foreach(MID ${NODE_INTERNAL_MIDS})
    # Check platform TLM / global TLM (0x0800 - 0x087F)
    if(MID GREATER_EQUAL 0x0800 AND MID LESS_EQUAL 0x087F)
        message(FATAL_ERROR "node_defs: internal MID ${MID} collides with cFE reserved TLM range (0x0800-0x087F)")
    endif()
    # Check platform CMD / global CMD (0x1800 - 0x187F)
    if(MID GREATER_EQUAL 0x1800 AND MID LESS_EQUAL 0x187F)
        message(FATAL_ERROR "node_defs: internal MID ${MID} collides with cFE reserved CMD range (0x1800-0x187F)")
    endif()
endforeach()
