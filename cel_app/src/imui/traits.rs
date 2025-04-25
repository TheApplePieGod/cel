use cel_renderer::renderer::Coord;

pub trait Sizable {
    fn size(self, size: Coord) -> Self;
}

pub trait Offsetable {
    fn offset(self, offset: Coord) -> Self;
}
