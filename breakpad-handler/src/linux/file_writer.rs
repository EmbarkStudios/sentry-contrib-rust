use crate::{minidump::Location, utils::to_byte_array};
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
};

#[derive(Copy, Clone)]
pub struct Reservation {
    pos: u64,
    size: u64,
}

impl Into<Location> for Reservation {
    fn into(self) -> Location {
        Location {
            rva: self.pos as u32,
            data_size: self.size as u32,
        }
    }
}

pub struct FileWriter<'file> {
    inner: &'file mut File,
    page_size: usize,
    pos: u64,
    len: u64,
}

impl<'file> FileWriter<'file> {
    pub fn new(file: &'file mut File) -> Self {
        Self {
            inner: file,
            page_size: crate::alloc::get_page_size(),
            pos: 0,
            len: 0,
        }
    }

    #[inline]
    pub fn flush(&mut self) -> Result<(), std::io::Error> {
        self.inner.flush()
    }

    #[inline]
    pub fn position(&self) -> u64 {
        self.pos
    }

    pub fn reserve_raw(&mut self, size: u64) -> Result<Reservation, std::io::Error> {
        let unwritten = self.len - self.pos;
        if unwritten < size {
            // Alloc in page sizes
            let num_pages = size / self.page_size as u64 + 1;

            let new_len = self.len + num_pages * self.page_size as u64;
            self.inner.set_len(new_len as u64)?;

            self.len = new_len;
        }

        let pos = self.pos;
        self.pos += size;

        Ok(Reservation { pos, size })
    }

    #[inline]
    pub fn reserve<Kind: Sized>(&mut self) -> Result<MDItem<Kind>, std::io::Error> {
        let reservation = self.reserve_raw(std::mem::size_of::<Kind>() as u64)?;

        Ok(MDItem {
            reservation,
            _kind: PD,
        })
    }

    #[inline]
    pub fn reserve_array<Kind: Sized>(
        &mut self,
        count: usize,
    ) -> Result<MDArray<Kind>, std::io::Error> {
        let reservation = self.reserve_raw((std::mem::size_of::<Kind>() * count) as u64)?;
        Ok(MDArray {
            reservation,
            _kind: PD,
        })
    }

    #[inline]
    pub fn reserve_header_array<Header: Sized, Kind: Sized>(
        &mut self,
        count: usize,
    ) -> Result<MDHeaderArray<Header, Kind>, std::io::Error> {
        let to_reserve = std::mem::size_of::<Header>() + std::mem::size_of::<Kind>() * count;
        let reservation = self.reserve_raw(to_reserve as u64)?;

        Ok(MDHeaderArray {
            reservation,
            _header: PD,
            _kind: PD,
        })
    }

    /// Writes the specified buffer to the location where it was reserved
    pub fn write(
        &mut self,
        reservation: Reservation,
        offset: usize,
        buffer: &[u8],
    ) -> Result<(), std::io::Error> {
        let ret_pos = self.pos;

        self.inner
            .seek(SeekFrom::Start(reservation.pos + offset as u64))?;
        self.inner.write_all(buffer)?;
        self.inner.seek(SeekFrom::Start(ret_pos))?;

        Ok(())
    }
}

use std::marker::PhantomData as PD;

pub struct MDItem<Kind: Sized> {
    reservation: Reservation,
    _kind: PD<Kind>,
}

impl<Kind> MDItem<Kind> {
    #[inline]
    pub fn location(&self) -> Location {
        self.reservation.into()
    }

    #[inline]
    pub fn write(&self, item: Kind, fw: &mut FileWriter<'_>) -> Result<(), std::io::Error> {
        fw.write(self.reservation, 0, to_byte_array(&item))
    }
}

pub struct MDArray<Kind: Sized> {
    reservation: Reservation,
    _kind: PD<Kind>,
}

impl<Kind> MDArray<Kind> {
    #[inline]
    pub fn location(&self) -> Location {
        self.reservation.into()
    }

    #[inline]
    pub fn write(
        &self,
        index: usize,
        item: Kind,
        fw: &mut FileWriter<'_>,
    ) -> Result<(), std::io::Error> {
        fw.write(
            self.reservation,
            std::mem::size_of::<Kind>() * index,
            to_byte_array(&item),
        )
    }
}

pub struct MDHeaderArray<Header: Sized, Kind: Sized> {
    reservation: Reservation,
    _header: PD<Header>,
    _kind: PD<Kind>,
}

impl<Header, Kind> MDHeaderArray<Header, Kind> {
    #[inline]
    pub fn location(&self) -> Location {
        self.reservation.into()
    }

    #[inline]
    pub fn write_header(
        &self,
        header: Header,
        fw: &mut FileWriter<'_>,
    ) -> Result<(), std::io::Error> {
        fw.write(self.reservation, 0, to_byte_array(&header))
    }

    #[inline]
    pub fn write(
        &self,
        index: usize,
        item: Kind,
        fw: &mut FileWriter<'_>,
    ) -> Result<(), std::io::Error> {
        fw.write(
            self.reservation,
            std::mem::size_of::<Header>() + std::mem::size_of::<Kind>() * index,
            to_byte_array(&item),
        )
    }
}
