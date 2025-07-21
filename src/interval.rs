#[derive(Debug, Clone, Copy)]
pub struct Interval {
    pub min: f32,
    pub max: f32,
}

impl Interval {
    pub const EMPTY: Interval = Interval {
        min: f32::INFINITY,
        max: f32::NEG_INFINITY,
    };

    pub const UNIVERSE: Interval = Interval {
        min: f32::NEG_INFINITY,
        max: f32::INFINITY,
    };

    pub fn new(min: f32, max: f32) -> Self {
        Interval { min, max }
    }

    pub fn center(&self) -> f32 {
        (self.min + self.max) * 0.5
    }

    pub fn enclosing(a: Interval, b: Interval) -> Self {
        Interval::new(f32::min(a.min, b.min), f32::max(a.max, b.max))
    }

    pub fn size(&self) -> f32 {
        self.max - self.min
    }

    pub fn contains(&self, value: f32) -> bool {
        self.min <= value && value <= self.max
    }

    pub fn surrounds(&self, value: f32) -> bool {
        self.min < value && value < self.max
    }

    pub fn expand(&self, delta: f32) -> Self {
        let padding = delta / 2.;
        return Interval::new(self.min - padding, self.max + padding);
    }
}

impl Default for Interval {
    fn default() -> Self {
        Interval::EMPTY
    }
}
