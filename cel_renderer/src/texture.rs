use std::{ffi::c_void, ptr::null};

pub struct Texture<T> {
    id: u32,
    width: u32,
    height: u32,
    channels: u32,
    pixels: Vec<T>,
    data_type: gl::types::GLenum,
    format: gl::types::GLenum,
    internal_format: gl::types::GLenum
}

impl<T: Default + Copy> Texture<T> {
    pub fn new(width: u32, height: u32, channels: u32, float: bool, initial_data: Option<&[T]>) -> Result<Self, &str> {
        let data: Vec<T> = match initial_data {
            Some(d) => d.to_vec(),
            None => vec![Default::default(); (width * height * channels) as usize]
        };
        let format = Texture::<T>::get_format_from_channels(channels);
        let internal_format = Texture::<T>::get_internal_format_from_channels(channels, float);
        let data_type = match float {
            true => gl::FLOAT,
            false => gl::UNSIGNED_BYTE
        };

        if format.is_none() || internal_format.is_none() {
            return Err("Invalid channel count");
        }

        let mut tex_id = 0;
        unsafe {
            gl::GenTextures(1, &mut tex_id);
            gl::BindTexture(gl::TEXTURE_2D, tex_id);
            gl::TexStorage2D(gl::TEXTURE_2D, 1, internal_format.unwrap(), width as i32, height as i32);
            gl::TexSubImage2D(
                gl::TEXTURE_2D, 
                0, 
                0, 
                0, 
                width as i32, 
                height as i32, 
                format.unwrap(), 
                data_type,
                data.as_ptr() as _
            );
        }

        Ok(Self {
            id: tex_id,
            width,
            height,
            channels,
            pixels: data,
            data_type,
            format: format.unwrap(),
            internal_format: internal_format.unwrap()
        })
    }

    fn get_format_from_channels(channels: u32) -> Option<gl::types::GLenum> {
        match channels {
            1 => Some(gl::RED),
            2 => Some(gl::RG),
            3 => Some(gl::RGB),
            4 => Some(gl::RGBA),
            _ => None
        }
    }

    fn get_internal_format_from_channels(channels: u32, float: bool) -> Option<gl::types::GLenum> {
        if float {
            match channels {
                1 => Some(gl::R32F),
                2 => Some(gl::RG32F),
                3 => Some(gl::RGB32F),
                4 => Some(gl::RGBA32F),
                _ => None
            }
        } else {
            match channels {
                1 => Some(gl::R8),
                2 => Some(gl::RG8),
                3 => Some(gl::RGB8),
                4 => Some(gl::RGBA8),
                _ => None
            }
        }
    }

    pub fn clear(&mut self, color: &[T]) {
        let size = (self.width * self.height * self.channels) as usize;
        for i in 0..size {
            self.pixels[i] = color[i % self.channels as usize];
        }

        unsafe {
            gl::TexSubImage2D(
                gl::TEXTURE_2D, 
                0, 
                0,
                0, 
                self.width as i32, 
                self.height as i32,
                self.format,
                self.data_type,
                self.pixels.as_ptr() as *const c_void
            );
        }
    }

    pub fn update_pixel(&mut self, x_offset: u32, y_offset: u32, color: &[T]) {
        self.update_pixels(x_offset, y_offset, 1, 1, color);
    }

    pub fn update_pixels(&mut self, x_offset: u32, y_offset: u32, width: u32, height: u32, data: &[T]) {
        if x_offset + width > self.width { return; }
        if y_offset + height > self.height { return; }

        let mut data_index = 0;
        for i in 0..height {
            for j in 0..width * self.channels {
                let arr_index = ((y_offset + i) * self.width * self.channels + (x_offset * self.channels + j)) as usize;
                self.pixels[arr_index] = data[data_index];
                data_index += 1;
            }
        }

        unsafe {
            gl::TexSubImage2D(
                gl::TEXTURE_2D, 
                0, 
                x_offset as i32, 
                y_offset as i32, 
                width as i32, 
                height as i32, 
                self.format, 
                self.data_type,
                data.as_ptr() as _
            );
        }
    }

    pub fn get_pixel_index(&self, x: u32, y: u32) -> usize {
        ((x * self.channels) + (y * self.channels * self.width)) as usize
    }

    pub fn get_id(&self) -> u32 { self.id }
    pub fn get_size(&self) -> [u32; 2] { [self.width, self.height] }
    pub fn get_size_f32(&self) -> [f32; 2] { [self.width as f32, self.height as f32] }
    pub fn get_width(&self) -> u32 { self.width }
    pub fn get_height(&self) -> u32 { self.height }
    pub fn get_width_f32(&self) -> f32 { self.width as f32 }
    pub fn get_height_f32(&self) -> f32 { self.height as f32 }
    pub fn get_channels(&self) -> u32 { self.channels }
    pub fn get_pixels(&self) -> &Vec<T> { &self.pixels }
}

impl<T> Drop for Texture<T> {
    fn drop(&mut self) {
        let del: [u32; 1] = [self.id];
        unsafe {
            gl::DeleteTextures(1, del.as_ptr());
        }
    }
}
