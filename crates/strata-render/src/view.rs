/// Dimensions and pointer coordinates use physical pixels, not AppKit points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewTransform {
    pub zoom: f32,
    pub center_x: f32,
    pub center_y: f32,
}

pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;
pub const FIT_MARGIN: f32 = 0.9;

impl Default for ViewTransform {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center_x: 0.0,
            center_y: 0.0,
        }
    }
}

impl ViewTransform {
    pub fn is_valid(&self) -> bool {
        self.zoom.is_finite()
            && self.zoom > 0.0
            && self.center_x.is_finite()
            && self.center_y.is_finite()
    }
    pub fn fit(width: u32, height: u32, vp: Viewport) -> Self {
        if width == 0 || height == 0 || vp.width == 0 || vp.height == 0 {
            return Self::default();
        }
        Self {
            zoom: ((vp.width as f32 / width as f32).min(vp.height as f32 / height as f32)
                * FIT_MARGIN)
                .clamp(MIN_ZOOM, MAX_ZOOM),
            center_x: width as f32 / 2.0,
            center_y: height as f32 / 2.0,
        }
    }
    pub fn screen_to_image(&self, vp: Viewport, sx: f32, sy: f32) -> (f32, f32) {
        (
            self.center_x + (sx - vp.width as f32 / 2.0) / self.zoom,
            self.center_y + (sy - vp.height as f32 / 2.0) / self.zoom,
        )
    }
    pub fn image_to_screen(&self, vp: Viewport, x: f32, y: f32) -> (f32, f32) {
        (
            (x - self.center_x) * self.zoom + vp.width as f32 / 2.0,
            (y - self.center_y) * self.zoom + vp.height as f32 / 2.0,
        )
    }
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        let next = Self {
            center_x: self.center_x - dx / self.zoom,
            center_y: self.center_y - dy / self.zoom,
            ..*self
        };
        if next.is_valid() {
            *self = next;
        }
    }
    pub fn zoom_at(&mut self, vp: Viewport, sx: f32, sy: f32, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 || !self.is_valid() {
            return;
        }
        let (x, y) = self.screen_to_image(vp, sx, sy);
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let next = Self {
            zoom,
            center_x: x - (sx - vp.width as f32 / 2.0) / zoom,
            center_y: y - (sy - vp.height as f32 / 2.0) / zoom,
        };
        if next.is_valid() {
            *self = next;
        }
    }
    pub fn rescaled(&self, old: f32, new: f32) -> Self {
        if !old.is_finite() || !new.is_finite() || old <= 0.0 || new <= 0.0 {
            return *self;
        }
        let next = Self {
            zoom: self.zoom * new / old,
            ..*self
        };
        if next.is_valid() { next } else { *self }
    }
    /// Column-major matrix: unit image quad -> WebGPU NDC; image Y points down.
    pub fn image_to_ndc(&self, vp: Viewport, width: u32, height: u32) -> [[f32; 4]; 4] {
        let vw = vp.width.max(1) as f32;
        let vh = vp.height.max(1) as f32;
        [
            [2.0 * width as f32 * self.zoom / vw, 0.0, 0.0, 0.0],
            [0.0, -2.0 * height as f32 * self.zoom / vh, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [
                -2.0 * self.center_x * self.zoom / vw,
                2.0 * self.center_y * self.zoom / vh,
                0.0,
                1.0,
            ],
        ]
    }
}
