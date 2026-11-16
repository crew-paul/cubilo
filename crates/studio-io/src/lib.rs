//! Binary STL export. Coordinates are millimetres by application convention;
//! STL itself does not encode units.
pub mod project;
use std::collections::BTreeSet;
use std::io::{self, Write};
use studio_geometry::{cross, sub, Mesh};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// Validate geometry and f32 conversion before writing any bytes.
pub fn write_binary_stl(mut writer: impl Write, mesh: &Mesh) -> io::Result<()> {
    mesh.validate().map_err(invalid)?;
    let count = u32::try_from(mesh.triangles().len()).map_err(|_| invalid("too many triangles"))?;
    let vertices: Vec<[f32; 3]> = mesh
        .vertices()
        .iter()
        .map(|p| p.map(|x| x as f32))
        .collect();
    let mut unique = BTreeSet::new();
    for p in &vertices {
        if p.iter().any(|v| !v.is_finite()) {
            return Err(invalid("STL coordinate overflow"));
        }
        // Canonicalise signed zero before checking conversion collisions.
        let key = p.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
        if !unique.insert(key) {
            return Err(invalid("STL precision merges vertices"));
        }
    }
    let normals: Vec<[f32; 3]> = mesh
        .triangles()
        .iter()
        .map(|&[a, b, c]| {
            let [a, b, c] = [a, b, c].map(|i| vertices[i].map(f64::from));
            let normal = cross(sub(b, a), sub(c, a));
            let length = normal.iter().map(|v| v * v).sum::<f64>().sqrt();
            if !length.is_finite() || length == 0.0 {
                return Err(invalid("STL precision collapses triangle"));
            }
            Ok(normal.map(|v| (v / length) as f32))
        })
        .collect::<io::Result<_>>()?;
    let mut header = [0u8; 80];
    let label = b"Cubilo CPU geometry PoC; millimetres";
    header[..label.len()].copy_from_slice(label);
    writer.write_all(&header)?;
    writer.write_all(&count.to_le_bytes())?;
    for (triangle, normal) in mesh.triangles().iter().zip(normals) {
        for value in normal {
            writer.write_all(&value.to_le_bytes())?;
        }
        for &index in triangle {
            for value in vertices[index] {
                writer.write_all(&value.to_le_bytes())?;
            }
        }
        writer.write_all(&0u16.to_le_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_geometry::{BoxParams, Hollow};
    #[test]
    fn binary_layout_and_normals() {
        let mesh = BoxParams {
            rounded: Default::default(),
            width: 40.0,
            depth: 30.0,
            height: 50.0,
            hollow: Hollow::default(),
        }
        .mesh()
        .unwrap();
        let mut bytes = Vec::new();
        write_binary_stl(&mut bytes, &mesh).unwrap();
        assert!(bytes[..80].starts_with(b"Cubilo"));
        let count = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert_eq!(count, mesh.triangles().len());
        assert_eq!(bytes.len(), 84 + 50 * count);
        for (record, triangle) in bytes[84..].as_chunks::<50>().0.iter().zip(mesh.triangles()) {
            let floats: Vec<f32> = record[..48]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            assert!((floats[..3].iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-6);
            for (actual, &index) in floats[3..].as_chunks::<3>().0.iter().zip(triangle) {
                assert_eq!(*actual, mesh.vertices()[index].map(|v| v as f32));
            }
            assert_eq!(&record[48..], &[0, 0]);
        }
    }
    #[test]
    fn rejects_precision_loss_before_output_and_propagates_io_error() {
        let mesh = BoxParams {
            rounded: Default::default(),
            width: 1_000_000.0,
            depth: 20.0,
            height: 20.0,
            hollow: Hollow {
                enabled: true,
                wall: 0.0001,
                bottom: 1.0,
                ..Hollow::default()
            },
        }
        .mesh()
        .unwrap();
        let mut bytes = Vec::new();
        assert_eq!(
            write_binary_stl(&mut bytes, &mesh).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(bytes.is_empty());
        let mesh = BoxParams {
            rounded: Default::default(),
            width: 20.0,
            depth: 20.0,
            height: 20.0,
            hollow: Hollow::default(),
        }
        .mesh()
        .unwrap();
        assert_eq!(
            write_binary_stl(&mut [0u8; 10][..], &mesh)
                .unwrap_err()
                .kind(),
            io::ErrorKind::WriteZero
        );
    }
}

/// Export to a same-directory temporary file; failed validation/I/O leaves no final file.
/// Existing user files are never silently overwritten.
pub fn save_binary_stl_new(path: &std::path::Path, mesh: &Mesh) -> io::Result<()> {
    save_binary_stl_atomic(path, mesh, false, None)
}
pub fn save_binary_stl_atomic(
    path: &std::path::Path,
    mesh: &Mesh,
    overwrite: bool,
    cancel: Option<&studio_geometry::boolean::CancelToken>,
) -> io::Result<()> {
    let check_cancel = || {
        if cancel.is_some_and(|c| c.is_cancelled()) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "내보내기가 취소되었습니다",
            ))
        } else {
            Ok(())
        }
    };
    check_cancel()?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    write_binary_stl(&mut temporary, mesh)?;
    temporary.as_file().sync_all()?;
    check_cancel()?;
    if overwrite {
        temporary.persist(path).map_err(|e| e.error)?;
    } else {
        temporary.persist_noclobber(path).map_err(|e| e.error)?;
    }
    Ok(())
}
#[cfg(test)]
mod export_tests {
    use super::*;
    use studio_geometry::{BoxParams, Hollow};
    #[test]
    fn atomic_export_preserves_existing_file_and_cleans_failed_output() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sample.stl");
        let mesh = BoxParams {
            rounded: Default::default(),
            width: 20.0,
            depth: 20.0,
            height: 20.0,
            hollow: Hollow::default(),
        }
        .mesh()
        .unwrap();
        save_binary_stl_new(&path, &mesh).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert!(save_binary_stl_new(&path, &mesh).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let invalid = BoxParams {
            rounded: Default::default(),
            width: 1_000_000.0,
            depth: 20.0,
            height: 20.0,
            hollow: Hollow {
                enabled: true,
                wall: 0.0001,
                ..Hollow::default()
            },
        }
        .mesh()
        .unwrap();
        let failed = directory.path().join("failed.stl");
        assert!(save_binary_stl_new(&failed, &invalid).is_err());
        assert!(!failed.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use studio_geometry::{boolean::CancelToken, BoxParams, Hollow};
    #[test]
    fn cancelled_export_does_not_replace_original() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("original.stl");
        std::fs::write(&path, b"original").unwrap();
        let mesh = BoxParams {
            width: 20.0,
            depth: 20.0,
            height: 20.0,
            hollow: Hollow::default(),
            rounded: Default::default(),
        }
        .mesh()
        .unwrap();
        let token = CancelToken::new();
        token.cancel();
        assert!(save_binary_stl_atomic(&path, &mesh, true, Some(&token)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
    }
}
