use windows::Win32::System::SystemInformation::{
    GetLogicalProcessorInformationEx, RelationProcessorCore,
    SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

/// 每个逻辑处理器的效率等级（Intel 混合架构：P 核等级高于 E 核；
/// 非混合架构全为同一等级）。失败时返回空数组。
pub fn core_efficiency_classes() -> Vec<u8> {
    unsafe {
        let mut len: u32 = 0;
        let _ = GetLogicalProcessorInformationEx(RelationProcessorCore, None, &mut len);
        if len == 0 {
            return Vec::new();
        }
        let mut buf = vec![0u8; len as usize];
        if GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            Some(buf.as_mut_ptr() as *mut SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX),
            &mut len,
        )
        .is_err()
        {
            return Vec::new();
        }

        let mut classes = vec![0u8; 64];
        let mut max_idx = 0usize;
        let mut off = 0usize;
        while off + 8 <= len as usize {
            let rec = &*(buf.as_ptr().add(off) as *const SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX);
            if rec.Size == 0 {
                break;
            }
            if rec.Relationship == RelationProcessorCore {
                let proc_rel = &rec.Anonymous.Processor;
                let eff = proc_rel.EfficiencyClass;
                // 单处理器组场景取首个组掩码（>64 线程的多组机器暂不细分）
                let mask = proc_rel.GroupMask[0].Mask;
                for bit in 0..64usize {
                    if mask & (1usize << bit) != 0 {
                        classes[bit] = eff;
                        max_idx = max_idx.max(bit);
                    }
                }
            }
            off += rec.Size as usize;
        }
        classes.truncate(max_idx + 1);
        classes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_cover_all_logical_cores() {
        let classes = core_efficiency_classes();
        println!("efficiency classes: {classes:?}");
        assert!(!classes.is_empty());
        let sys = sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::nothing()
                .with_cpu(sysinfo::CpuRefreshKind::nothing().with_cpu_usage()),
        );
        assert_eq!(classes.len(), sys.cpus().len());
    }
}
