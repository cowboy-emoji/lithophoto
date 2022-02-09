use clap::{App, Arg};
use image::{DynamicImage, GenericImageView};
use indicatif::ProgressBar;
use indicatif::ProgressIterator;
use std::fs::File;

struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    fn to_le_bytes(&self) -> [u8; 12] {
        let mut bytes = [0; 12];
        bytes[0..4].copy_from_slice(&self.x.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.y.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.z.to_le_bytes());
        bytes
    }
}

impl Copy for Vec3 {}

impl Clone for Vec3 {
    fn clone(&self) -> Self {
        Vec3 {
            x: self.x,
            y: self.y,
            z: self.z,
        }
    }
}

struct Triangle {
    normal: Vec3,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
}

type Mesh = Vec<Triangle>;

fn generate_stl_mesh<T: std::io::Write>(m: Mesh, w: &mut T) {
    // Write 80 byte header
    for _ in 0..80 {
        w.write(&[0]).unwrap();
    }
    // Write number of triangles (u32)
    let num_triangles = m.len() as u32;
    let num_triangles_bytes = num_triangles.to_le_bytes();
    w.write(&num_triangles_bytes).unwrap();

    println!("Writing STL...");
    let bar = ProgressBar::new(num_triangles as u64);

    // Write triangles
    for t in m {
        let normal = t.normal;
        let v0 = t.v0;
        let v1 = t.v1;
        let v2 = t.v2;

        let normal_bytes = normal.to_le_bytes();
        w.write(&normal_bytes).unwrap();

        let v0_bytes = v0.to_le_bytes();
        w.write(&v0_bytes).unwrap();

        let v1_bytes = v1.to_le_bytes();
        w.write(&v1_bytes).unwrap();

        let v2_bytes = v2.to_le_bytes();
        w.write(&v2_bytes).unwrap();

        // Write attribute byte count (u16)
        w.write(&[0, 0]).unwrap();

        bar.inc(1);
    }
}

fn get_pixel_brightness(r: u8, g: u8, b: u8) -> f32 {
    // Use the standard formula for brightness
    let brightness = (r as f32 * 0.299) + (g as f32 * 0.587) + (b as f32 * 0.114);
    brightness / 255.0
}

fn image_to_mesh(img: &DynamicImage, mesh_width: f32, mesh_thickness: f32, contrast: f32) -> Mesh {
    let mut mesh = Mesh::new();

    let (width, height) = img.dimensions();

    let brightness_to_mm =
        |brightness: f32| mesh_thickness - (brightness * contrast * mesh_thickness);

    let pixel_coord_to_mm = |val| val as f32 * mesh_width / (width as f32);
    let get_thickness_vec3 = |x, y| {
        let pixel = img.get_pixel(x, height - y - 1);
        let brightness = get_pixel_brightness(pixel[0], pixel[1], pixel[2]);
        let mm = brightness_to_mm(brightness);
        Vec3 {
            x: pixel_coord_to_mm(x),
            y: pixel_coord_to_mm(y),
            z: mm,
        }
    };
    println!("Computing brightness...");
    let thickness = (0..height)
        .progress()
        .map(|y| {
            (0..width)
                .map(|x| get_thickness_vec3(x, y))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    println!("Generating mesh...");

    let normal = |v0: Vec3, v1: Vec3, v2: Vec3| Vec3 {
        x: (v1.y - v0.y) * (v2.z - v0.z) - (v1.z - v0.z) * (v2.y - v0.y),
        y: (v1.z - v0.z) * (v2.x - v0.x) - (v1.x - v0.x) * (v2.z - v0.z),
        z: (v1.x - v0.x) * (v2.y - v0.y) - (v1.y - v0.y) * (v2.x - v0.x),
    };

    let add_quad = |mesh: &mut Mesh, v0: Vec3, v1: Vec3, v2: Vec3, v3: Vec3| {
        let normal = normal(v0, v1, v2);
        mesh.push(Triangle { normal, v0, v1, v2 });
        mesh.push(Triangle {
            normal,
            v0,
            v1: v2,
            v2: v3,
        });
    };

    let add_quad_with_normal =
        |mesh: &mut Mesh, v0: Vec3, v1: Vec3, v2: Vec3, v3: Vec3, n: Vec3| {
            mesh.push(Triangle {
                normal: n,
                v0,
                v1,
                v2,
            });
            mesh.push(Triangle {
                normal: n,
                v0,
                v1: v2,
                v2: v3,
            });
        };

    // Create front face by tesselating a plane with the given thickness
    for y in (0..height - 1).progress() {
        for x in 0..width - 1 {
            let (x, y) = (x as usize, y as usize);
            let v0 = thickness[y][x];
            let v1 = thickness[y][x + 1];
            let v2 = thickness[y + 1][x + 1];
            let v3 = thickness[y + 1][x];

            add_quad(&mut mesh, v0, v1, v2, v3);
        }
    }

    // Create back face
    add_quad_with_normal(
        &mut mesh,
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        Vec3 {
            x: pixel_coord_to_mm(width - 1),
            y: 0.0,
            z: 0.0,
        },
        Vec3 {
            x: pixel_coord_to_mm(width - 1),
            y: pixel_coord_to_mm(height - 1),
            z: 0.0,
        },
        Vec3 {
            x: 0.0,
            y: pixel_coord_to_mm(height - 1),
            z: 0.0,
        },
        Vec3 {
            x: 0.0,
            y: 0.0,
            z: -1.0,
        },
    );

    let (width, height) = (width as usize, height as usize);

    // Create left and right faces
    for x in 0..width - 1 {
        let a0 = thickness[0][x];
        let a1 = thickness[0][x + 1];
        let a2 = Vec3 {
            x: a0.x,
            y: a0.y,
            z: 0.0,
        };
        let a3 = Vec3 {
            x: a1.x,
            y: a1.y,
            z: 0.0,
        };

        let b0 = thickness[height - 1][x + 1];
        let b1 = thickness[height - 1][x];
        let b2 = Vec3 {
            x: b0.x,
            y: b0.y,
            z: 0.0,
        };
        let b3 = Vec3 {
            x: b1.x,
            y: b1.y,
            z: 0.0,
        };

        // Quad with (a0, a1, a2, a3)
        let normal = Vec3 {
            x: -1.0,
            y: 0.0,
            z: 0.0,
        };
        add_quad_with_normal(&mut mesh, a0, a1, a3, a2, normal);

        // Quad with (b0, b1, b2, b3)

        let normal = Vec3 {
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        add_quad_with_normal(&mut mesh, b0, b1, b3, b2, normal);
    }

    // Create top and bottom faces
    for y in 0..height - 1 {
        let a0 = thickness[y][0];
        let a1 = thickness[y + 1][0];
        let a2 = Vec3 {
            x: a0.x,
            y: a0.y,
            z: 0.0,
        };
        let a3 = Vec3 {
            x: a1.x,
            y: a1.y,
            z: 0.0,
        };
        let b0 = thickness[y][width - 1];
        let b1 = thickness[y + 1][width - 1];
        let b2 = Vec3 {
            x: b0.x,
            y: b0.y,
            z: 0.0,
        };
        let b3 = Vec3 {
            x: b1.x,
            y: b1.y,
            z: 0.0,
        };

        // Quad with (a0, a1, a2, a3)
        let normal = Vec3 {
            x: 0.0,
            y: -1.0,
            z: 0.0,
        };
        add_quad_with_normal(&mut mesh, a0, a1, a3, a2, normal);

        // Quad with (b0, b1, b2, b3)
        let normal = Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        };
        add_quad_with_normal(&mut mesh, b0, b1, b3, b2, normal);
    }

    mesh
}

fn main() {
    let matches = App::new("lithophoto")
        .version("0.1.0")
        .about("Generates STL lithophane models from images")
        .arg(
            Arg::with_name("input")
                .short("i")
                .long("input")
                .value_name("FILE")
                .help("Sets the input PNG file to use")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("output")
                .short("o")
                .long("output")
                .value_name("FILE")
                .help("Save the output STL file to the given path")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("width")
                .short("w")
                .long("width")
                .value_name("WIDTH")
                .help("Sets the width of the model in mm. Height is calculated based on the aspect ratio of the image")
                .takes_value(true)
                .default_value("100"),
        )
        .arg(
            Arg::with_name("thickness")
                .short("t")
                .long("thickness")
                .value_name("THICKNESS")
                .help("Sets the thickness of the model in mm")
                .takes_value(true)
                .default_value("10"),
        )
        .arg(
            Arg::with_name("contrast")
                .short("c")
                .long("contrast")
                .value_name("CONTRAST")
                .help("Value between 0 and 1 controlling how much of the thickness is exposed.")
                .takes_value(true)
                .default_value("0.5"),
        )
        
        .get_matches();

    // Parse arguments
    let input_path = matches.value_of("input").unwrap();
    let output_path = matches.value_of("output").unwrap();
    let width = matches.value_of("width").unwrap().parse::<f32>().unwrap();
    let thickness = matches.value_of("thickness").unwrap().parse::<f32>().unwrap();
    let contrast = matches.value_of("contrast").unwrap().parse::<f32>().unwrap();
    // Load image
    let image = image::open(input_path).unwrap();

    // Mesh dimensions in mm
    let mesh_width = width;

    // Generate mesh
    let mesh = image_to_mesh(&image, mesh_width, thickness, contrast);

    // Write STL file
    let mut output_file = File::create(output_path).unwrap();
    generate_stl_mesh(mesh, &mut output_file);
}
