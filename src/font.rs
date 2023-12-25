use rust_fontconfig::{FcFontCache, FcPattern};
use msdf::{GlyphLoader, Projection, SDFTrait};
use ttf_parser::Face;
use std::{fs::File, io::{BufReader, Read}, collections::{HashMap, hash_map::Entry}};
use std::num::NonZeroUsize;
use lru::LruCache;
use crate::texture::Texture;

const ATLAS_SIZE: u32 = 2048;
const MSDF_SIZE: u32 = 32;
const MSDF_RANGE: f32 = 2.0;

pub struct GlyphData {
    pixels: Vec<f32>,
    atlas_index: u32
}

pub struct Font {
    name: String,
    font_data: Vec<u8>,
    glyph_cache: HashMap<char, GlyphData>,
    glyph_lru: LruCache<char, u32>,
    atlas_free_list: u32,
    atlas_tex: Texture<f32>
}

impl GlyphData {
    pub fn get_pixels(&self) -> &[f32] {
        &self.pixels
    }
}

impl Font {
    pub fn new(cache: &FcFontCache, name: &str) -> Result<Self, String> {
        let result = cache.query(&FcPattern {
            name: Some(name.to_string()),
            ..Default::default()
        });

        if result.is_none() {
            return Err("Font not found!".to_string());
        }

        let file = File::open(&result.unwrap().path).unwrap();
        let mut reader = BufReader::new(file);
        
        let mut font_data = vec![];
        reader.read_to_end(&mut font_data).unwrap();

        // Validate
        match Face::parse(font_data.as_slice(), 0) {
            Err(err) => return Err(err.to_string()),
            _ => {}
        }

        // Generate font atlas data
        let max_glyphs = (ATLAS_SIZE / MSDF_SIZE) * (ATLAS_SIZE / MSDF_SIZE);
        let atlas_tex = Texture::new(ATLAS_SIZE, ATLAS_SIZE, 4, true, None)?;

        Ok(Self {
            name: name.to_string(),
            font_data,
            glyph_cache: Default::default(),
            glyph_lru: LruCache::new(NonZeroUsize::new(max_glyphs as usize).unwrap()),
            atlas_free_list: 0,
            atlas_tex
        })
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_atlas_tex(&self) -> &Texture<f32> {
        &self.atlas_tex
    }

    pub fn get_atlas_size(&self) -> u32 {
        ATLAS_SIZE
    }

    pub fn get_glyph_size(&self) -> u32 {
        MSDF_SIZE
    }

    pub fn get_pixel_range(&self) -> f32 {
        MSDF_RANGE
    }

    pub fn get_glyph_texcoord(&mut self, key: char) -> [f32; 2] {
        let atlas_index: u32;

        match self.glyph_lru.get(&key) {
            Some(v) => {
                // Already present, return immediately
                let offset = Self::convert_atlas_index_to_offset(*v);
                return Self::convert_atlas_offset_to_texcoord(offset);
            },
            None => {
                // Pull from free list first
                if self.atlas_free_list < self.glyph_lru.cap().get() as u32 {
                    atlas_index = self.atlas_free_list;
                    self.atlas_free_list += 1;
                    self.glyph_lru.push(key, atlas_index);
                } else {
                    // LRU Will always be full here since the LRU is the same
                    // size as the free list, so we can safely evict
                    let lru = self.glyph_lru.pop_lru().unwrap();
                    atlas_index = lru.1;

                    self.glyph_lru.put(key, atlas_index);
                }
            }
        }

        // Code reaching this point indicates atlas tex needs to be updated

        let glyph_data = self.glyph_cache.entry(key).or_insert_with(|| {
            // TODO: handle invalid glyphs
            let face = Face::parse(self.font_data.as_slice(), 0).unwrap();
            let glyph_index = face.glyph_index(key).unwrap();
            let shape = face.load_shape(glyph_index).unwrap();
            let colored_shape = shape.color_edges_ink_trap(3.0);
            let global_bb = face.global_bounding_box();

            let projection = Projection {
                scale: [
                    MSDF_SIZE as f64 / global_bb.width() as f64,
                    MSDF_SIZE as f64 / global_bb.height() as f64
                ].into(),
                //scale: [1.0, 1.0].into(),
                translation: [0.0, 0.0].into()
            };

            let msdf_config = Default::default();
            let msdf = colored_shape.generate_mtsdf(
                MSDF_SIZE, MSDF_SIZE,
                MSDF_RANGE.into(),
                &projection,
                &msdf_config
            );
            
            //let pixels: Vec<u8> = msdf.image().into_iter().map(|&x| (x * 255.0) as u8).collect();
            GlyphData {
                pixels: msdf.image().to_vec(),
                atlas_index: 0
            }
        });

        let offset = Self::convert_atlas_index_to_offset(atlas_index);
        self.atlas_tex.update_pixels(
            offset[0], offset[1],
            MSDF_SIZE, MSDF_SIZE,
            &glyph_data.pixels
        );

        Self::convert_atlas_offset_to_texcoord(offset)
    }

    fn convert_atlas_index_to_offset(index: u32) -> [u32; 2] {
        let tex_index = index * MSDF_SIZE;
        let x = tex_index % ATLAS_SIZE;
        let y = tex_index / ATLAS_SIZE;
        [x, y]
    }

    fn convert_atlas_offset_to_texcoord(offset: [u32; 2]) -> [f32; 2] {
        let atlas_f32 = ATLAS_SIZE as f32;
        [offset[0] as f32 / atlas_f32, offset[1] as f32 / atlas_f32]
    }
}
