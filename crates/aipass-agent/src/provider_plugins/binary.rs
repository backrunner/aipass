//! Validate the selected asset's executable format and CPU before showing consent.
use object::{BinaryFormat, Object, ObjectKind, ObjectSegment, SegmentFlags};

fn executable(bytes: &[u8], format: BinaryFormat) -> bool {
    let Ok(file) = object::File::parse(bytes) else {
        return false;
    };
    if file.format() != format || !file.is_64() || !file.is_little_endian() {
        return false;
    }
    let executable_kind = file.kind() == ObjectKind::Executable
        || (format == BinaryFormat::Elf && file.kind() == ObjectKind::Dynamic);
    if !executable_kind || (format != BinaryFormat::MachO && file.entry() == 0) {
        return false;
    }
    let mut has_code = false;
    for segment in file.segments() {
        let Ok(data) = segment.data() else {
            return false;
        };
        let execute = match segment.flags() {
            SegmentFlags::Elf { p_type, p_flags } => {
                p_type == object::elf::PT_LOAD && p_flags.contains(object::elf::PF_X)
            }
            SegmentFlags::MachO { initprot, .. } => {
                initprot.contains(object::macho::VM_PROT_EXECUTE)
            }
            SegmentFlags::Coff { characteristics } => {
                characteristics.contains(object::pe::IMAGE_SCN_MEM_EXECUTE)
            }
            _ => false,
        };
        if execute && !data.is_empty() {
            // Mach-O also supports LC_UNIXTHREAD entry points.
            has_code |= format == BinaryFormat::MachO
                || file
                    .entry()
                    .checked_sub(segment.address())
                    .is_some_and(|offset| offset < data.len() as u64);
        }
    }
    has_code
}

fn word16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}
fn word32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}
fn big(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
}
fn macho(bytes: &[u8], cpu: u32) -> bool {
    bytes.starts_with(b"\xcf\xfa\xed\xfe")
        && bytes.len() >= 32
        && word32(bytes, 4) == Some(cpu)
        && word32(bytes, 12) == Some(2)
        && executable(bytes, BinaryFormat::MachO)
}
fn mac(bytes: &[u8], cpu: u32) -> bool {
    if macho(bytes, cpu) {
        return true;
    }
    let stride = match big(bytes) {
        Some(0xcafebabe) => 20,
        Some(0xcafebabf) => 32,
        _ => return false,
    };
    let Some(count) = bytes
        .get(4..)
        .and_then(big)
        .filter(|n| (1..=32).contains(n))
    else {
        return false;
    };
    let Some(arches) = bytes.get(8..8 + stride * count as usize) else {
        return false;
    };
    arches.chunks_exact(stride).any(|a| {
        if big(a) != Some(cpu) {
            return false;
        }
        let (offset, size) = if stride == 20 {
            (
                u32::from_be_bytes(a[8..12].try_into().unwrap()) as u64,
                u32::from_be_bytes(a[12..16].try_into().unwrap()) as u64,
            )
        } else {
            (
                u64::from_be_bytes(a[8..16].try_into().unwrap()),
                u64::from_be_bytes(a[16..24].try_into().unwrap()),
            )
        };
        offset
            .checked_add(size)
            .and_then(|end| bytes.get(usize::try_from(offset).ok()?..usize::try_from(end).ok()?))
            .is_some_and(|slice| macho(slice, cpu))
    })
}
pub(super) fn compatible(bytes: &[u8], entry: &str, os: &str, arch: &str) -> bool {
    let (elf, pe, mach) = match arch {
        "x86_64" => (62, 0x8664, 0x01000007),
        "aarch64" => (183, 0xaa64, 0x0100000c),
        _ => return false,
    };
    match os {
        "macos" => mac(bytes, mach),
        "linux" => {
            bytes.len() >= 64
                && bytes.starts_with(b"\x7fELF\x02\x01\x01")
                && matches!(word16(bytes, 16), Some(2 | 3))
                && word16(bytes, 18) == Some(elf)
                && executable(bytes, BinaryFormat::Elf)
        }
        "windows" if entry.to_ascii_lowercase().ends_with(".exe") && bytes.starts_with(b"MZ") => {
            let Some(header) = word32(bytes, 60).and_then(|offset| bytes.get(offset as usize..))
            else {
                return false;
            };
            header.starts_with(b"PE\0\0")
                && word16(header, 4) == Some(pe)
                && word16(header, 22).is_some_and(|flags| flags & 0x2002 == 2)
                && word16(header, 24) == Some(0x20b)
                && executable(bytes, BinaryFormat::Pe)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::compatible;

    #[test]
    fn file_headers_without_loadable_code_are_rejected() {
        let mut mach = [0u8; 32];
        mach[..4].copy_from_slice(b"\xcf\xfa\xed\xfe");
        mach[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
        mach[12..16].copy_from_slice(&2u32.to_le_bytes());
        assert!(!compatible(&mach, "demo", "macos", "aarch64"));

        let mut elf = [0u8; 64];
        elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        elf[16..18].copy_from_slice(&2u16.to_le_bytes());
        elf[18..20].copy_from_slice(&62u16.to_le_bytes());
        assert!(!compatible(&elf, "demo", "linux", "x86_64"));

        let mut pe = [0u8; 128];
        pe[..2].copy_from_slice(b"MZ");
        pe[60..64].copy_from_slice(&64u32.to_le_bytes());
        pe[64..68].copy_from_slice(b"PE\0\0");
        pe[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[86..88].copy_from_slice(&2u16.to_le_bytes());
        pe[88..90].copy_from_slice(&0x20bu16.to_le_bytes());
        assert!(!compatible(&pe, "demo.exe", "windows", "x86_64"));
    }
}
