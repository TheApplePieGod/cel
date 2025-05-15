use cel_renderer::renderer::{Coord, Renderer};

pub trait Sizable {
    fn set_size(&mut self, size: Coord);
    fn get_size(&self) -> &Coord;
}

pub trait Offsetable {
    fn set_offset(&mut self, offset: Coord);
    fn get_offset(&self) -> &Coord;
}

pub trait Renderable {
    fn render(&self, renderer: &mut Renderer);
}
