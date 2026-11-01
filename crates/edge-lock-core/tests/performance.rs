//! Opt-in, reproducible local profiling; all input and output are disposable.
use edge_lock_core::{AssetFilter, ExportConfig, Library, OperationContext};
use std::{
    fs,
    io::{self, Read},
    path::Path,
    time::{Duration, Instant},
};

#[test]
#[ignore = "opt-in filesystem profiling: creates 1000 assets and a 128 MiB streamed snapshot"]
fn profiles_catalog_pagination_and_large_streamed_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source");
    fs::create_dir(&input).unwrap();
    fs::write(input.join("model.blend"), b"synthetic performance fixture").unwrap();
    let library = Library::open(temp.path().join("library")).unwrap();
    let context = OperationContext::default();
    let start = Instant::now();
    let mut first_variant = None;
    for index in 0..1000 {
        let asset = library
            .import_folder(
                &input,
                &format!("Asset {index:04}"),
                ExportConfig::default(),
                &context,
            )
            .unwrap();
        if index == 0 {
            first_variant = Some(asset.variants[0].clone());
        }
    }
    let import_duration = start.elapsed();
    let mut page_times = Vec::new();
    for index in 0..20 {
        let start = Instant::now();
        let page = library
            .list_assets(AssetFilter {
                sort: "name".into(),
                offset: index * 50,
                limit: 50,
                ..Default::default()
            })
            .unwrap();
        page_times.push(start.elapsed());
        assert_eq!(page.total, 1000);
        assert_eq!(page.items.len(), 50);
    }
    page_times.sort();
    let variant = first_variant.unwrap();
    let mut output =
        fs::File::create(Path::new(&variant.workspace_path).join("large.bin")).unwrap();
    io::copy(&mut io::repeat(42).take(128 * 1024 * 1024), &mut output).unwrap();
    output.sync_all().unwrap();
    drop(output);
    let peak_before = peak_working_set();
    let start = Instant::now();
    let snapshot = library
        .create_snapshot(&variant.id, "128 MiB fixture", &context)
        .unwrap();
    let snapshot_duration = start.elapsed();
    assert!(snapshot.logical_bytes >= 128 * 1024 * 1024);
    let peak_after = peak_working_set();
    println!(
        "1000 imports: {import_duration:?}; page50 median: {:?}; page50 p95: {:?}; 128MiB snapshot: {snapshot_duration:?}; process peak before/after: {peak_before:?}/{peak_after:?}",
        page_times[10], page_times[18]
    );
    // Deliberately generous regression bounds. Printed timings provide the actual
    // measurement and are not advertised as guarantees on other hardware.
    assert!(page_times[18] < Duration::from_secs(5));
    #[cfg(windows)]
    assert!(
        peak_after.unwrap().saturating_sub(peak_before.unwrap()) < 96 * 1024 * 1024,
        "capture unexpectedly buffered most of the 128 MiB input"
    );
}

#[cfg(windows)]
fn peak_working_set() -> Option<usize> {
    #[repr(C)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak_working_set: usize,
        working_set: usize,
        peak_paged: usize,
        paged: usize,
        peak_non_paged: usize,
        non_paged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let size = std::mem::size_of::<Counters>() as u32;
    let mut counters = Counters {
        cb: size,
        faults: 0,
        peak_working_set: 0,
        working_set: 0,
        peak_paged: 0,
        paged: 0,
        peak_non_paged: 0,
        non_paged: 0,
        pagefile: 0,
        peak_pagefile: 0,
    };
    // Windows documents -1 as the current-process pseudo-handle. Counters is a
    // correctly sized writable PROCESS_MEMORY_COUNTERS structure for this call.
    let success =
        unsafe { GetProcessMemoryInfo((-1isize) as *mut std::ffi::c_void, &mut counters, size) };
    (success != 0).then_some(counters.peak_working_set)
}

#[cfg(not(windows))]
fn peak_working_set() -> Option<usize> {
    None
}
