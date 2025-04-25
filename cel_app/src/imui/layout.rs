use cel_renderer::renderer::{Coord, Renderer};
use chrono::offset;

use super::traits::*;

pub enum LayoutMode {
    Fit(usize), // Max items 
    Grow,
    //Clip,
}

pub struct Layout {
    mode: LayoutMode,
    size: Coord,
    offset: Coord,
    bg_color: Option<[f32; 4]>,
    num_items: u32,
    cur_offset_screen: f32,
}

impl Sizable for Layout {
    fn set_size(&mut self, size: Coord) {
        self.size = size;
    }

    fn get_size(&self) -> &Coord {
        &self.size
    }
}

impl Offsetable for Layout {
    fn set_offset(&mut self, offset: Coord) {
        self.offset = offset;
    }

    fn get_offset(&self) -> &Coord {
        &self.offset
    }
}

impl Layout {
    pub fn new() -> Self {
        Layout {
            mode: LayoutMode::Grow,
            size: Coord::Px([0.0, 0.0]),
            offset: Coord::Px([0.0, 0.0]),
            bg_color: None,
            num_items: 0,
            cur_offset_screen: 0.0,
        }
    }

    pub fn size(mut self, size: Coord) -> Self {
        Sizable::set_size(&mut self, size);
        self
    }

    pub fn offset(mut self, offset: Coord) -> Self {
        Offsetable::set_offset(&mut self, offset);
        self
    }

    pub fn mode(mut self, mode: LayoutMode) -> Self {
        self.mode = mode;
        self
    }

    /*
    // TODO: background rendering
    pub fn bg_color(mut self, color: [f32; 4]) -> Self {
        self.bg_color = Some(color);
        self
    }
    */

    pub fn render_next_item<T: Sizable + Offsetable + Renderable>(
        mut self,
        renderer: &mut Renderer,
        item: &mut T
    ) -> Self {
        let size_screen = self.size.screen(renderer);
        let offset_screen = self.offset.screen(renderer);
        match self.mode {
            LayoutMode::Fit(max_items) => {
                let elem_size = [size_screen[0], size_screen[1] / max_items as f32];
                let elem_offset = [offset_screen[0], offset_screen[1] + self.cur_offset_screen];
                item.set_size(Coord::Screen(elem_size));
                item.set_offset(Coord::Screen(elem_offset));
                item.render(renderer);

                self.cur_offset_screen += elem_size[1];
            },
            LayoutMode::Grow => {
                let elem_size = *item.get_size();
                let elem_offset = [offset_screen[0], offset_screen[1] + self.cur_offset_screen];
                item.set_offset(Coord::Screen(elem_offset));
                item.render(renderer);

                self.cur_offset_screen += elem_size.screen(renderer)[1];
            }
            //LayoutMode::Clip => {}
        }

        self.num_items += 1;

        self
    }
}
