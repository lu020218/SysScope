//! PDH（Performance Data Helper）封装：替代 Win32_PerfFormattedData_* WMI 查询。
//! WMI 格式化计数器每次查询固定 250-540ms（提供者内部等待采样窗口），
//! PDH 将全部计数器注册进一个查询句柄，每拍一次 PdhCollectQueryData（~1-5ms），
//! 由 PDH 基于两次 collect 的差分计算速率——这正是高频采样的正确姿势。

use windows::core::PCWSTR;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 单个 PDH 查询句柄：所有计数器挂在其下，每个采样拍 collect 一次
pub struct PdhQuery {
    h: isize,
}

impl PdhQuery {
    pub fn new() -> Option<Self> {
        let mut h = 0isize;
        let st = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut h) };
        if st != 0 {
            eprintln!("[sysscope] PdhOpenQuery failed: 0x{st:08X}");
            return None;
        }
        Some(PdhQuery { h })
    }

    /// 用英文计数器路径注册（免本地化问题，如 "\\Memory\\Committed Bytes"）；
    /// 机器缺少该计数器对象时返回 None，调用方按不可用降级
    pub fn add(&self, english_path: &str) -> Option<PdhCounter> {
        let w = wide(english_path);
        let mut c = 0isize;
        let st = unsafe { PdhAddEnglishCounterW(self.h, PCWSTR(w.as_ptr()), 0, &mut c) };
        if st != 0 {
            eprintln!("[sysscope] PDH add failed 0x{st:08X}: {english_path}");
            return None;
        }
        Some(PdhCounter { h: c })
    }

    /// 每个采样拍调用一次；速率类计数器需要两次 collect 后才有有效值
    pub fn collect(&self) {
        unsafe {
            let _ = PdhCollectQueryData(self.h);
        }
    }
}

impl Drop for PdhQuery {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.h);
        }
    }
}

pub struct PdhCounter {
    h: isize,
}

impl PdhCounter {
    /// 单实例计数器的当前格式化值；数据尚无效（首拍）时返回 None
    pub fn value(&self) -> Option<f64> {
        let mut v = PDH_FMT_COUNTERVALUE::default();
        let st = unsafe { PdhGetFormattedCounterValue(self.h, PDH_FMT_DOUBLE, None, &mut v) };
        // CStatus: 0=VALID 1=NEW_DATA，其余视为无效
        (st == 0 && v.CStatus <= 1).then_some(unsafe { v.Anonymous.doubleValue })
    }

    /// 通配实例计数器的 (实例名, 值) 数组；无效实例被过滤
    pub fn array(&self) -> Vec<(String, f64)> {
        unsafe {
            let mut buf_size = 0u32;
            let mut count = 0u32;
            let st = PdhGetFormattedCounterArrayW(
                self.h,
                PDH_FMT_DOUBLE,
                &mut buf_size,
                &mut count,
                None,
            );
            if st != PDH_MORE_DATA || buf_size == 0 {
                return Vec::new();
            }
            let mut buf = vec![0u8; buf_size as usize];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(
                self.h,
                PDH_FMT_DOUBLE,
                &mut buf_size,
                &mut count,
                Some(items),
            ) != 0
            {
                return Vec::new();
            }
            (0..count as usize)
                .filter_map(|i| {
                    let item = &*items.add(i);
                    if item.FmtValue.CStatus > 1 {
                        return None;
                    }
                    let name = item.szName.to_string().ok()?;
                    Some((name, item.FmtValue.Anonymous.doubleValue))
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdh_scalar_and_wildcard_work() {
        let q = PdhQuery::new().expect("open query");
        let commit = q.add("\\Memory\\Committed Bytes").expect("scalar counter");
        let disks = q.add("\\PhysicalDisk(*)\\% Idle Time").expect("wildcard counter");
        q.collect();
        std::thread::sleep(std::time::Duration::from_millis(400));
        q.collect();
        let v = commit.value().expect("committed bytes valid");
        assert!(v > 1e8, "committed bytes implausible: {v}");
        let arr = disks.array();
        println!("disk instances: {arr:?}");
        assert!(!arr.is_empty(), "no disk instances");
        assert!(arr.iter().any(|(n, _)| n == "_Total"));
    }
}
