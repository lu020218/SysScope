use wmi::{COMLibrary, WMIConnection};

/// 进程内唯一的 WMI 查询入口：负责线程 COM 初始化与连接持有。
/// 各领域采集器只通过 query<T>() 使用，不直接接触 wmi crate。
pub struct WmiHub {
    conn: Option<WMIConnection>,
}

impl WmiHub {
    /// 必须在使用它的线程内创建（依赖线程 COM 环境）
    pub fn new() -> Self {
        Self::connect(None)
    }

    /// 连接非默认命名空间。存储相关的 MSFT_* 类不在 root\cimv2 里，
    /// 而在 root\Microsoft\Windows\Storage。
    pub fn with_namespace(namespace: &str) -> Self {
        Self::connect(Some(namespace))
    }

    fn connect(namespace: Option<&str>) -> Self {
        let conn = COMLibrary::new()
            .or_else(|_| Ok::<_, wmi::WMIError>(unsafe { COMLibrary::assume_initialized() }))
            .ok()
            .and_then(|com| match namespace {
                Some(ns) => WMIConnection::with_namespace_path(ns, com).ok(),
                None => WMIConnection::new(com).ok(),
            });
        if conn.is_none() {
            eprintln!(
                "[sysscope] WMI unavailable ({}), dependent metrics disabled",
                namespace.unwrap_or("default namespace")
            );
        }
        WmiHub { conn }
    }

    /// 按类型查询该 WMI 类的全部实例；不可用或失败时返回 None
    pub fn query<T: serde::de::DeserializeOwned>(&self) -> Option<Vec<T>> {
        self.conn.as_ref().and_then(|c| c.query::<T>().ok())
    }
}
