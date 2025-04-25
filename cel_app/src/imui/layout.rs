use cel_renderer::renderer::{Coord, Renderer};

use super::traits::{Offsetable, Sizable};

pub struct Layout {
    size: Coord,
    offset: Coord,
}

impl Sizable for Layout {
    fn size(mut self, size: Coord) -> Self {
        self.size = size;
        self
    }
}

impl Offsetable for Layout {
    fn offset(mut self, offset: Coord) -> Self {
        self.offset = offset;
        self
    }
}

impl Layout {
    pub fn new() -> Self {
        Layout {
            size: Coord::Px([0.0, 0.0]),
            offset: Coord::Px([0.0, 0.0]),
        }
    }

    pub fn size(self, size: Coord) -> Self {
        Sizable::size(self, size)
    }

    pub fn offset(mut self, offset: Coord) -> Self {
        Offsetable::offset(self, offset)
    }

    
}
