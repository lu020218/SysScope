use wmi::{COMLibrary, WMIConnection};

/// 进程内唯一的 WMI 查询入口：负责线程 COM 初始化与连接持有。
/// 各领域采集器只通过 query<T>() 使用，不直接接触 wmi crate。
pub struct WmiHub {
    conn: Option<WMIConnection>,
}

impl WmiHub {
    /// 必须在采样线程内创建（依赖线程 COM 环境）
    pub fn new() -> Self {
        let conn = COMLibrary::new()
            .or_else(|_| Ok::<_, wmi::WMIError>(unsafe { COMLibrary::assume_initialized() }))
            .ok()
            .and_then(|com| WMIConnection::new(com).ok());
        if conn.is_none() {
            eprintln!("[sysscope] WMI unavailable, dependent metrics disabled");
        }
        WmiHub { conn }
    }

    /// 按类型查询该 WMI 类的全部实例；不可用或失败时返回 None
    pub fn query<T: serde::de::DeserializeOwned>(&self) -> Option<Vec<T>> {
        self.conn.as_ref().and_then(|c| c.query::<T>().ok())
    }
}
