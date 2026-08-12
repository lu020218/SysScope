use windows::Win32::NetworkManagement::IpHelper::{GetTcpTable, GetUdpTable, MIB_TCPTABLE, MIB_UDPTABLE};

/// TCP 状态常量（MIB_TCP_STATE）
const TCP_STATE_LISTEN: u32 = 2;
const TCP_STATE_ESTAB: u32 = 5;
const TCP_STATE_TIME_WAIT: u32 = 11;

/// IPv4 TCP 连接分状态计数：(已建立, TIME_WAIT, 监听)
pub fn tcp_state_counts() -> (u32, u32, u32) {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetTcpTable(None, &mut size, false);
        if size == 0 {
            return (0, 0, 0);
        }
        let mut buf = vec![0u8; size as usize];
        if GetTcpTable(Some(buf.as_mut_ptr() as *mut MIB_TCPTABLE), &mut size, false) != 0 {
            return (0, 0, 0);
        }
        let table = &*(buf.as_ptr() as *const MIB_TCPTABLE);
        let rows = std::slice::from_raw_parts(table.table.as_ptr(), table.dwNumEntries as usize);
        let (mut estab, mut tw, mut listen) = (0u32, 0u32, 0u32);
        for row in rows {
            match row.Anonymous.dwState {
                TCP_STATE_ESTAB => estab += 1,
                TCP_STATE_TIME_WAIT => tw += 1,
                TCP_STATE_LISTEN => listen += 1,
                _ => {}
            }
        }
        (estab, tw, listen)
    }
}

/// IPv4 UDP 活动端点数
pub fn udp_endpoint_count() -> u32 {
    unsafe {
        let mut size: u32 = 0;
        let _ = GetUdpTable(None, &mut size, false);
        if size == 0 {
            return 0;
        }
        let mut buf = vec![0u8; size as usize];
        if GetUdpTable(Some(buf.as_mut_ptr() as *mut MIB_UDPTABLE), &mut size, false) != 0 {
            return 0;
        }
        (*(buf.as_ptr() as *const MIB_UDPTABLE)).dwNumEntries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_tables_are_readable() {
        let (estab, tw, listen) = tcp_state_counts();
        let udp = udp_endpoint_count();
        println!("tcp estab={estab} time_wait={tw} listen={listen}, udp endpoints={udp}");
        // 桌面系统必然有监听端口与 UDP 端点
        assert!(listen > 0, "expected some listening TCP ports");
        assert!(udp > 0, "expected some UDP endpoints");
    }
}
