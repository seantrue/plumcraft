//! How an array's elements are stored: the numpy dtypes the engines make, little-endian, as
//! `.npy` names them. Moved from `plumcraft-ui`'s grid viewer to the core (stage N1), where the
//! probe ([`crate::probe`]) and the viewers read with it.

/// How to read one element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dtype {
    U8,
    I8,
    Bool,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

impl Dtype {
    pub fn parse(s: &str) -> Option<Dtype> {
        Some(match s {
            "|u1" => Dtype::U8,
            "|i1" => Dtype::I8,
            "|b1" => Dtype::Bool,
            "<u2" => Dtype::U16,
            "<i2" => Dtype::I16,
            "<u4" => Dtype::U32,
            "<i4" => Dtype::I32,
            "<u8" => Dtype::U64,
            "<i8" => Dtype::I64,
            "<f4" => Dtype::F32,
            "<f8" => Dtype::F64,
            _ => return None,
        })
    }

    pub fn size(self) -> usize {
        match self {
            Dtype::U8 | Dtype::I8 | Dtype::Bool => 1,
            Dtype::U16 | Dtype::I16 => 2,
            Dtype::U32 | Dtype::I32 | Dtype::F32 => 4,
            Dtype::U64 | Dtype::I64 | Dtype::F64 => 8,
        }
    }

    pub fn integer(self) -> bool {
        !matches!(self, Dtype::F32 | Dtype::F64)
    }

    /// Element `i` of `bytes`, or `None` past the end.
    pub fn read(self, bytes: &[u8], i: usize) -> Option<f64> {
        let at = i.checked_mul(self.size())?;
        let b = bytes.get(at..at + self.size())?;
        let arr = |n: usize| -> [u8; 8] {
            let mut a = [0u8; 8];
            a[..n].copy_from_slice(&b[..n]);
            a
        };
        Some(match self {
            Dtype::U8 => f64::from(b[0]),
            Dtype::I8 => f64::from(b[0] as i8),
            Dtype::Bool => f64::from(u8::from(b[0] != 0)),
            Dtype::U16 => f64::from(u16::from_le_bytes([b[0], b[1]])),
            Dtype::I16 => f64::from(i16::from_le_bytes([b[0], b[1]])),
            Dtype::U32 => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Dtype::I32 => f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Dtype::U64 => u64::from_le_bytes(arr(8)) as f64,
            Dtype::I64 => i64::from_le_bytes(arr(8)) as f64,
            Dtype::F32 => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Dtype::F64 => f64::from_le_bytes(arr(8)),
        })
    }
}
