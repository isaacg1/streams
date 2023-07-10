use image::{ImageBuffer, RgbImage};
use rand::prelude::*;
use rand_distr::{Exp, LogNormal, Normal, StandardNormal};
use scarlet::prelude::*;
use scarlet::colors::CIELABColor;

use std::f64::consts::PI;

// Faucets create streams, streams move according to forces
#[derive(Clone, Copy, Default)]
struct ColorOffset {
    r: f64,
    g: f64,
    b: f64,
}
impl ColorOffset {
    fn to_rgb(&self, color_cap: f64) -> [u8; 3] {
        let length = (self.r.powi(2) + self.g.powi(2) + self.b.powi(2)).sqrt();
        let ratio = if length > color_cap {
            color_cap / length
        } else {
            1.0
        };

        let tightness = 1.0;
        let to_01 = &|f: f64| {
            let scale_f = f * ratio;
            0.5 * scale_f / (1.0 + scale_f.abs().powf(tightness)).powf(1.0 / tightness) + 0.5
        };
        let color = CIELABColor { l: to_01(self.r) * 100.0, a: to_01(self.g) * 255.0 - 128.0,
        b: to_01(self.b) * 255.0 - 128.0};
        let rgb = color.convert::<RGBColor>();
        [rgb.int_r(), rgb.int_g(), rgb.int_b()]
    }
    fn scale(&self, ratio: f64) -> Self {
        ColorOffset {
            r: self.r * ratio,
            g: self.g * ratio,
            b: self.b * ratio,
        }
    }
    fn add(&self, other: Self) -> Self {
        ColorOffset {
            r: self.r + other.r,
            g: self.g + other.g,
            b: self.b + other.b,
        }
    }
}

#[derive(Clone, Copy)]
struct Position {
    x: f64,
    y: f64,
}
impl Position {
    fn sample<R: Rng>(rng: &mut R, size: u32) -> Self {
        Position {
            x: rng.gen::<f64>() * size as f64,
            y: rng.gen::<f64>() * size as f64,
        }
    }
    fn sample_direction<R: Rng>(rng: &mut R) -> Self {
        let angle: f64 = rng.gen::<f64>() * 2.0 * PI;
        Position {
            x: angle.cos(),
            y: angle.sin(),
        }
    }
    fn add(&self, other: Self) -> Self {
        Position {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
    fn scale(&self, ratio: f64) -> Self {
        Position {
            x: self.x * ratio,
            y: self.y * ratio,
        }
    }
    fn to_pixels(&self, size: u32) -> (Option<usize>, Option<usize>) {
        let f_size = size as f64;
        let to_pixel = &|f: f64| {
            if f > 0.0 && f < f_size {
                Some(f as usize)
            } else {
                None
            }
        };
        (to_pixel(self.x), to_pixel(self.y))
    }
    fn length(&self) -> f64 {
        (self.x.powi(2) + self.y.powi(2)).sqrt()
    }
}

// Center and spreads are normally distributed
// Stream color normally distributed according to that center + spread.
// Stream position and velocity normally distributed
// Spreads exponetially distributed
struct Faucet {
    color_center: ColorOffset,
    color_spreads: ColorOffset,
    position: Position,
    position_spreads: Position,
    velocity_spreads: Position,
}

// Age is number of pixels crossed
// Decay rate is exponentially distibuted around a decay center, ~1/size
// Intentity is downscaled by e^-(decay_rate * age)
// Removed after age * decay_rate ~ 10
struct Stream {
    color: ColorOffset,
    decay_rate: f64,
    position: Position,
    velocity: Position,
}

enum ForceKind {
    Inward,
    Outward,
    Linear(Position),
}
impl ForceKind {
    fn sample<R: Rng>(rng: &mut R) -> Self {
        let main: f64 = rng.gen();
        if main < 0.333 {
            ForceKind::Inward
        } else if main < 0.666 {
            ForceKind::Outward
        } else {
            ForceKind::Linear(Position::sample_direction(rng))
        }
    }
}

// Lognormal force strength, spread distribution
// Force spreads normally
struct Force {
    kind: ForceKind,
    strength: f64,
    position: Position,
    spread: f64,
}
impl Force {
    fn apply(&self, target: Position) -> Position {
        let offset = target.add(self.position.scale(-1.0));
        let distance = offset.length();
        let num_devs = distance / self.spread;
        let push = self.strength/self.spread * (-num_devs.powi(2) / 2.0).exp();
        let dir: Position = match self.kind {
            ForceKind::Inward => offset.scale(-1.0 / distance),
            ForceKind::Outward => offset.scale(1.0 / distance),
            ForceKind::Linear(dir) => dir,
        };
        dir.scale(push)
    }
}

#[derive(Debug)]
struct Params {
    size: u32,
    seed: u64,
    num_forces: usize,
    force_strength_dist: LogNormal<f64>,
    force_spread_dist: LogNormal<f64>,
    num_faucets: usize,
    faucet_color_center_dist: Normal<f64>,
    faucet_color_spread_dist: Exp<f64>,
    faucet_position_spread_dist: Exp<f64>,
    faucet_velocity_spread_dist: Exp<f64>,
    num_streams: usize,
    decay_dist: Exp<f64>,
    max_decay_factor: f64,
    velocity_cap: f64,
    color_cap: f64,
}
fn draw(params: Params) -> RgbImage {
    let mut rng = StdRng::seed_from_u64(params.seed);
    // Create forces
    let forces: Vec<Force> = (0..params.num_forces)
        .map(|_| {
            let position = Position::sample(&mut rng, params.size);
            let kind = ForceKind::sample(&mut rng);
            let strength = params.force_strength_dist.sample(&mut rng);
            let spread = params.force_spread_dist.sample(&mut rng);
            Force {
                kind,
                strength,
                spread,
                position,
            }
        })
        .collect();
    // Create faucets
    let faucets: Vec<Faucet> = (0..params.num_faucets)
        .map(|_| {
            let color_center = ColorOffset {
                r: params.faucet_color_center_dist.sample(&mut rng),
                g: params.faucet_color_center_dist.sample(&mut rng),
                b: params.faucet_color_center_dist.sample(&mut rng),
            };
            let color_spreads = ColorOffset {
                r: params.faucet_color_spread_dist.sample(&mut rng),
                g: params.faucet_color_spread_dist.sample(&mut rng),
                b: params.faucet_color_spread_dist.sample(&mut rng),
            };
            let position = Position::sample(&mut rng, params.size);
            let position_spreads = Position {
                x: params.faucet_position_spread_dist.sample(&mut rng),
                y: params.faucet_position_spread_dist.sample(&mut rng),
            };
            let velocity_spreads = Position {
                x: params.faucet_velocity_spread_dist.sample(&mut rng),
                y: params.faucet_velocity_spread_dist.sample(&mut rng),
            };
            Faucet {
                color_center,
                color_spreads,
                position,
                position_spreads,
                velocity_spreads,
            }
        })
        .collect();
    // Sample streams
    let streams: Vec<Stream> = (0..params.num_streams)
        .map(|_| {
            let faucet_index = rng.gen_range(0..params.num_faucets);
            let faucet = &faucets[faucet_index];
            let color = ColorOffset {
                r: faucet.color_center.r
                    + faucet.color_spreads.r * rng.sample::<f64, StandardNormal>(StandardNormal),
                g: faucet.color_center.g
                    + faucet.color_spreads.g * rng.sample::<f64, StandardNormal>(StandardNormal),
                b: faucet.color_center.b
                    + faucet.color_spreads.b * rng.sample::<f64, StandardNormal>(StandardNormal),
            };
            let position = Position {
                x: faucet.position.x
                    + faucet.position_spreads.x * rng.sample::<f64, StandardNormal>(StandardNormal),
                y: faucet.position.y
                    + faucet.position_spreads.y * rng.sample::<f64, StandardNormal>(StandardNormal),
            };
            let velocity = Position {
                x: faucet.velocity_spreads.x * rng.sample::<f64, StandardNormal>(StandardNormal),
                y: faucet.velocity_spreads.y * rng.sample::<f64, StandardNormal>(StandardNormal),
            };
            let decay_rate = params.decay_dist.sample(&mut rng);
            Stream {
                color,
                position,
                velocity,
                decay_rate,
            }
        })
        .collect();
    // Create image to draw into - x then y.
    let mut grid: Vec<Vec<ColorOffset>> =
        vec![vec![Default::default(); params.size as usize]; params.size as usize];
    // Draw streams
    for mut stream in streams {
        let max_age = (params.max_decay_factor / stream.decay_rate) as u64;
        let mut age = 0;
        while age < max_age
            && !(stream.position.x < -(params.size as f64))
            && !(stream.position.x > 2.0 * params.size as f64)
            && !(stream.position.y < -(params.size as f64))
            && !(stream.position.y > 2.0 * params.size as f64)
        {
            let old_age = age;
            // Draw connecting line
            let norm = stream.velocity.x.abs().max(stream.velocity.y.abs());
            let base_offset = stream.velocity.scale(1.0 / norm as f64);
            let num_pixels = norm as usize;
            for i in 1..=num_pixels {
                let offset = base_offset.scale(i as f64);
                let current_position = stream.position.add(offset);
                if let (Some(pixel_x), Some(pixel_y)) = current_position.to_pixels(params.size) {
                    let intensity = (-stream.decay_rate * age as f64).exp();
                    let color = stream.color.scale(intensity);
                    grid[pixel_x][pixel_y] = grid[pixel_x][pixel_y].add(color);
                }
                age += 1;
            }
            // Update position
            stream.position = stream.position.add(stream.velocity);
            // Update age at least a minimum amount
            if age == old_age {
                age += 1;
            }
            // Update velocity via forces
            for force in &forces {
                let velocity_update = force.apply(stream.position);
                stream.velocity = stream.velocity.add(velocity_update);
            }
            // Cap velocity
            if stream.velocity.length() > params.velocity_cap {
                stream.velocity = stream
                    .velocity
                    .scale(params.velocity_cap / stream.velocity.length())
            }
        }
    }
    // Convert to final image
    let mut img: RgbImage = ImageBuffer::new(params.size, params.size);
    for (x, row) in grid.iter().enumerate() {
        for (y, color) in row.iter().enumerate() {
            img.put_pixel(x as u32, y as u32, image::Rgb(color.to_rgb(params.color_cap)))
        }
    }
    img
}

fn log_dist(center: f64, mult_spread: f64) -> LogNormal<f64> {
    LogNormal::new(center.ln(), mult_spread.ln()).expect("Valid dist")
}

fn main() {
    let size = 1000;
    let params = Params {
        size,
        seed: 0,
        num_forces: 200,
        force_spread_dist: log_dist(200.0, 2.0),
        force_strength_dist: log_dist(10.0, 2.0),
        num_faucets: 40,
        faucet_color_center_dist: Normal::new(0.0, 0.03).expect("Valid dist"),
        faucet_color_spread_dist: Exp::new(1.0 / 0.03).expect("Valid dist"),
        faucet_position_spread_dist: Exp::new(1.0 / 80.0).expect("Valid dist"),
        faucet_velocity_spread_dist: Exp::new(1.0 / 1.0).expect("Valid dist"),
        num_streams: 100000,
        decay_dist: Exp::new(size as f64).expect("Valid dist"),
        max_decay_factor: 10.0,
        velocity_cap: 40.0,
        color_cap: 2.0,
    };
    dbg!(&params);
    let num_entries = std::fs::read_dir(".").expect("Valid").count();
    let image = draw(params);
    let filename: String = format!("img-{}-{}.png", num_entries, size);
    image.save(&filename).expect("Saved");
    println!("{}", filename);
}
