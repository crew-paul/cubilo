use std::{
    env,
    error::Error,
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::PathBuf,
};
use studio_geometry::{BoxParams, CylinderParams, Hollow};
use studio_io::write_binary_stl;

mod design;
mod gizmo;
mod gui;
mod resize;
mod sketch;
mod worker;

fn main() -> Result<(), Box<dyn Error>> {
    if env::args_os().len() == 1 {
        return gui::run().map_err(|e| e.to_string().into());
    }
    let mut args = env::args_os().skip(1);
    let flag = args.next();
    let path = args.next();
    if flag.as_deref() != Some(std::ffi::OsStr::new("--output-dir"))
        || path.is_none()
        || args.next().is_some()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: studio-app --output-dir <directory>",
        )
        .into());
    }
    let dir = PathBuf::from(path.ok_or("missing output directory")?);
    let hollow = Hollow {
        enabled: true,
        ..Hollow::default()
    };
    let samples = [
        (
            "hollow-box.stl",
            BoxParams {
                rounded: Default::default(),
                width: 60.0,
                depth: 50.0,
                height: 80.0,
                hollow,
            }
            .mesh()?,
        ),
        (
            "hollow-cylinder.stl",
            CylinderParams {
                rounded: Default::default(),
                taper: Default::default(),
                diameter: 60.0,
                height: 80.0,
                segments: 96,
                hollow,
            }
            .mesh()?,
        ),
    ];
    fs::create_dir_all(&dir)?;
    // Refuse to replace existing user files.
    for (name, mesh) in samples {
        let path = dir.join(name);
        let mut output = BufWriter::new(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?,
        );
        write_binary_stl(&mut output, &mesh)?;
        output.flush()?;
        println!("{} ({} triangles)", path.display(), mesh.triangles().len());
    }
    Ok(())
}
