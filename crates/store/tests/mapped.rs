//! An array over `MAX_READ` is mapped, not read: its numbers are there, at no cost in memory.

use plumcraft_proto::Handle;
use plumcraft_proto::data::{BlobMeta, DataRef, Form, Locator};
use plumcraft_store::{Bytes, MAX_READ, Store, key_for};

/// A `.npy` v1 header for `<f4` in C order.
fn npy_header(shape: &[u64]) -> Vec<u8> {
    let dims = shape.iter().map(|d| format!("{d}, ")).collect::<String>();
    let mut dict = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({dims}), }}");
    while (10 + dict.len() + 1) % 64 != 0 {
        dict.push(' ');
    }
    dict.push('\n');
    let mut out = b"\x93NUMPY\x01\x00".to_vec();
    out.extend_from_slice(&(dict.len() as u16).to_le_bytes());
    out.extend_from_slice(dict.as_bytes());
    out
}

#[test]
fn an_array_over_the_read_limit_is_mapped_and_reads_true() {
    let store = Store::new(plumcraft_scratch::dir("mapped"));
    let n = (MAX_READ as usize) / 4 + 1024;
    let mut bytes = vec![0u8; n * 4];
    bytes[(n - 1) * 4..].copy_from_slice(&0.5f32.to_le_bytes());
    let shape = vec![1, n as u64];
    // As an engine with its own writer publishes it (this store's own limit is lower).
    let handle = Handle::md5_nd1("<f4", &shape, &bytes).unwrap();
    let path = store.path_for(&handle).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, [npy_header(&shape).as_slice(), &bytes].concat()).unwrap();
    let at = vec![Locator::Commonkit { key: key_for(&handle) }];
    let form = Form::Array { dtype: "<f4".into(), shape: shape.clone(), space: None, names: Vec::new(), table: false };
    let r = DataRef { handle: Some(handle), meta: BlobMeta { media: "application/x-npy".into(), size: bytes.len() as u64, form, aliases: Vec::new() }, at };
    let b = store.resolve(&r, true).expect("mapped");
    assert!(matches!(b.bytes, Bytes::Mapped(_)), "{:?}", b.bytes);
    assert_eq!(b.bytes.len(), bytes.len());
    assert_eq!(b.bytes[(n - 1) * 4..], 0.5f32.to_le_bytes());
    // not another array's file: a DataRef that says otherwise is a miss
    let mut wrong = r.clone();
    wrong.meta.form = Form::Array { dtype: "<f4".into(), shape: vec![n as u64, 1], space: None, names: Vec::new(), table: false };
    assert!(store.resolve(&wrong, true).is_none());
    // and off this machine, nothing
    assert!(store.resolve(&r, false).is_none());
}
