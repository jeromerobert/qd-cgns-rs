use core::ffi::CStr;
use std::ffi::{c_void, CString};
use std::fmt::Debug;
use std::sync::{Mutex, MutexGuard};

use cgns_sys::DataType_t::RealDouble;
use cgns_sys::ZoneType_t::Unstructured;
use cgns_sys::{
    cg_array_write, cg_base_write, cg_biter_read, cg_biter_write, cg_close, cg_coord_info,
    cg_coord_read, cg_coord_write, cg_elements_read, cg_get_error, cg_golist, cg_open,
    cg_section_read, cg_section_write, cg_ziter_write, cg_zone_read, cg_zone_write, DataType_t,
    CG_MODE_MODIFY, CG_MODE_READ, CG_MODE_WRITE,
};

pub use cgns_sys::ElementType_t;
pub struct Error(i32);
type Result<T> = std::result::Result<T, Error>;

pub enum Mode {
    Read,
    Write,
    Modify,
}

pub trait CgnsDataType {
    const SYS: DataType_t::Type;
}

pub struct GotoContext<'a>(MutexGuard<'a, ()>);

impl<'a> GotoContext<'a> {
    pub fn array_write<T: CgnsDataType>(
        &self,
        arrayname: &str,
        dimensions: &[i32],
        data: &[T],
    ) -> Result<()> {
        let arrayname = CString::new(arrayname).unwrap();
        assert_eq!(
            dimensions.iter().copied().reduce(|a, v| a * v).unwrap(),
            data.len() as i32
        );
        let e = unsafe {
            cg_array_write(
                arrayname.as_ptr(),
                T::SYS,
                dimensions.len() as i32,
                dimensions.as_ptr(),
                data.as_ptr().cast::<std::ffi::c_void>(),
            )
        };
        if e == 0 {
            Ok(())
        } else {
            Err(e.into())
        }
    }
}

impl CgnsDataType for i32 {
    const SYS: DataType_t::Type = DataType_t::Integer;
}

impl From<Mode> for i32 {
    fn from(m: Mode) -> i32 {
        match m {
            Mode::Read => CG_MODE_READ as i32,
            Mode::Write => CG_MODE_WRITE as i32,
            Mode::Modify => CG_MODE_MODIFY as i32,
        }
    }
}

impl From<i32> for Error {
    fn from(code: i32) -> Self {
        Error(code)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = unsafe { CStr::from_ptr(cg_get_error()) };
        write!(f, "{} (error {})", msg.to_str().unwrap(), self.0)
    }
}

static CGNS_MUTEX: Mutex<()> = Mutex::new(());

pub fn open(path: &str, mode: Mode) -> Result<File> {
    let _l = CGNS_MUTEX.lock().unwrap();
    let mut fd: i32 = 0;
    let path = CString::new(path).unwrap();
    let f = unsafe { cg_open(path.as_ptr(), mode.into(), &mut fd) };
    if f == 0 {
        Ok(File(fd))
    } else {
        Err(f.into())
    }
}

pub struct File(i32);
#[derive(Copy, Clone)]
pub struct Base(i32);
impl Base {
    #[must_use]
    pub fn new(arg: i32) -> Base {
        Base(arg)
    }
}
#[derive(Copy, Clone)]
pub struct Zone(i32);

fn raw_to_string(buf: &[u8]) -> String {
    let nulpos = buf.iter().position(|&r| r == 0).unwrap();
    CStr::from_bytes_with_nul(&buf[0..=nulpos])
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

#[derive(Default)]
pub struct SectionInfo {
    pub section_name: String,
    pub typ: ElementType_t::Type,
    pub start: i32,
    pub end: i32,
    pub nbndry: i32,
}

impl SectionInfo {
    #[must_use]
    pub fn new(typ: ElementType_t::Type, end: i32) -> Self {
        Self {
            section_name: "Elem".to_owned(),
            typ,
            start: 0,
            end,
            nbndry: 0,
        }
    }
}

impl File {
    pub fn close(&mut self) -> Result<()> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let f = unsafe { cg_close(self.0) };
        if f == 0 {
            Ok(())
        } else {
            Err(f.into())
        }
    }

    pub fn biter_write(&mut self, base: Base, base_iter_name: &str, n_steps: i32) -> Result<()> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let base_iter_name = CString::new(base_iter_name).unwrap();
        let e = unsafe { cg_biter_write(self.0, base.0, base_iter_name.as_ptr(), n_steps) };
        if e == 0 {
            Ok(())
        } else {
            Err(e.into())
        }
    }

    pub fn biter_read(&mut self, base: Base) -> Result<(String, i32)> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let mut n_steps = 0;
        let mut name = [0_u8; 33];
        let e = unsafe { cg_biter_read(self.0, base.0, name.as_mut_ptr().cast(), &mut n_steps) };
        if e == 0 {
            Ok((raw_to_string(&name), n_steps))
        } else {
            Err(e.into())
        }
    }

    pub fn golist(&self, base: Base, labels: &[&str], index: &[i32]) -> Result<GotoContext> {
        let l = CGNS_MUTEX.lock().unwrap();
        let labels: Vec<_> = labels.iter().map(|&s| CString::new(s).unwrap()).collect();
        let mut labels_ptr: Vec<_> = labels.iter().map(|s| s.as_ptr() as *mut i8).collect();
        let e = unsafe {
            cg_golist(
                self.0,
                base.0,
                labels.len() as i32,
                labels_ptr.as_mut_ptr(),
                index.as_ptr() as *mut i32,
            )
        };
        if e == 0 {
            Ok(GotoContext(l))
        } else {
            Err(e.into())
        }
    }

    // https://cgns.github.io/CGNS_docs_current/sids/timedep.html
    pub fn ziter_write(&mut self, base: Base, zone: Zone, zone_iter_name: &str) -> Result<()> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let zone_iter_name = CString::new(zone_iter_name).unwrap();
        let e = unsafe { cg_ziter_write(self.0, base.0, zone.0, zone_iter_name.as_ptr()) };
        if e == 0 {
            Ok(())
        } else {
            Err(e.into())
        }
    }

    // https://cgns.github.io/CGNS_docs_current/midlevel/structural.html
    pub fn base_write(&mut self, basename: &str, cell_dim: i32, phys_dim: i32) -> Result<Base> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let basename = CString::new(basename).unwrap();
        let mut b: i32 = 0;
        let e = unsafe { cg_base_write(self.0, basename.as_ptr(), cell_dim, phys_dim, &mut b) };
        if e == 0 {
            Ok(Base(b))
        } else {
            Err(e.into())
        }
    }
    pub fn zone_write(
        &mut self,
        base: Base,
        zonename: &str,
        vertex_size: i32,
        cell_size: i32,
        boundary_size: i32,
    ) -> Result<Zone> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let zonename = CString::new(zonename).unwrap();
        let mut z: i32 = 0;
        let size = [vertex_size, cell_size, boundary_size];
        let e = unsafe {
            cg_zone_write(
                self.0,
                base.0,
                zonename.as_ptr(),
                size.as_ptr(),
                Unstructured,
                &mut z,
            )
        };
        if e == 0 {
            Ok(Zone(z))
        } else {
            Err(e.into())
        }
    }

    // https://cgns.github.io/CGNS_docs_current/midlevel/grid.html
    pub fn coord_write(
        &mut self,
        base: Base,
        zone: Zone,
        coordname: &str,
        coord: &[f64],
    ) -> Result<()> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let coordname = CString::new(coordname).unwrap();
        let mut c = 0;
        let e = unsafe {
            cg_coord_write(
                self.0,
                base.0,
                zone.0,
                RealDouble,
                coordname.as_ptr(),
                coord.as_ptr().cast::<c_void>(),
                &mut c,
            )
        };
        if e == 0 {
            Ok(())
        } else {
            Err(e.into())
        }
    }

    pub fn zone_read(&self, base: Base, zone: Zone) -> Result<(String, Vec<i32>)> {
        let mut v = Vec::with_capacity(3);
        let mut buf = [0_u8; 64];
        let err = unsafe {
            cg_zone_read(
                self.0,
                base.0,
                zone.0,
                buf.as_mut_ptr().cast(),
                v.as_mut_ptr(),
            )
        };
        if err == 0 {
            Ok((raw_to_string(&buf), v))
        } else {
            Err(err.into())
        }
    }

    pub fn coord_info(&self, base: Base, zone: Zone, c: i32) -> Result<(DataType_t::Type, String)> {
        let mut datatype = DataType_t::Integer;
        let mut raw_name = [0_u8; 64];
        let err = unsafe {
            cg_coord_info(
                self.0,
                base.0,
                zone.0,
                c,
                &mut datatype,
                raw_name.as_mut_ptr().cast(),
            )
        };
        if err == 0 {
            Ok((datatype, raw_to_string(&raw_name)))
        } else {
            Err(err.into())
        }
    }

    pub fn coord_read(
        &self,
        base: Base,
        zone: Zone,
        coordname: &str,
        range_min: i32,
        range_max: i32,
        coord_array: &mut [f64],
    ) -> Result<()> {
        let p = CString::new(coordname).unwrap();
        let err = unsafe {
            cg_coord_read(
                self.0,
                base.0,
                zone.0,
                p.as_ptr(),
                RealDouble,
                &range_min,
                &range_max,
                coord_array.as_mut_ptr().cast(),
            )
        };
        if err == 0 {
            Ok(())
        } else {
            Err(err.into())
        }
    }

    pub fn section_write(
        &mut self,
        base: Base,
        zone: Zone,
        args: &SectionInfo,
        elements: &[i32],
    ) -> Result<()> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let section_name = CString::new(args.section_name.clone()).unwrap();
        let mut c = 0;
        let e = unsafe {
            cg_section_write(
                self.0,
                base.0,
                zone.0,
                section_name.as_ptr(),
                args.typ,
                args.start,
                args.end,
                args.nbndry,
                elements.as_ptr(),
                &mut c,
            )
        };
        if e == 0 {
            Ok(())
        } else {
            Err(e.into())
        }
    }

    pub fn elements_read(
        &self,
        base: Base,
        zone: Zone,
        section: i32,
        elements: &mut [i32],
        parent_data: &mut [i32],
    ) -> Result<()> {
        let ptr = if parent_data.is_empty() {
            std::ptr::null_mut()
        } else {
            parent_data.as_mut_ptr()
        };
        let e = unsafe {
            cg_elements_read(self.0, base.0, zone.0, section, elements.as_mut_ptr(), ptr)
        };
        if e == 0 {
            Ok(())
        } else {
            Err(e.into())
        }
    }

    pub fn section_read(
        &self,
        base: Base,
        zone: Zone,
        section: i32,
    ) -> Result<(SectionInfo, bool)> {
        let _l = CGNS_MUTEX.lock().unwrap();
        let mut info = SectionInfo::default();
        let mut parent_flag = 0_i32;
        let mut raw_name = [0_u8; 64];
        let e = unsafe {
            cg_section_read(
                self.0,
                base.0,
                zone.0,
                section,
                raw_name.as_mut_ptr().cast(),
                &mut info.typ,
                &mut info.start,
                &mut info.end,
                &mut info.nbndry,
                &mut parent_flag,
            )
        };
        if e == 0 {
            info.section_name = raw_to_string(&raw_name);
            Ok((info, parent_flag != 0))
        } else {
            Err(e.into())
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        self.close().unwrap();
    }
}
