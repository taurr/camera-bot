use anyhow::Result;
use opencv::{
    core::{Scalar, Vector, CV_32F},
    prelude::{Mat, MatTraitConst},
};
use tracing::instrument;

#[derive(Debug, Clone)]
pub struct AlphaImage {
    image_f: Mat,
    beta_f: Mat,
}

impl AlphaImage {
    pub fn new(rgba: Mat) -> Result<Self> {
        match prep_alpha_blend(rgba) {
            Ok((beta_f, image_f)) => Ok(Self { image_f, beta_f }),
            Err(e) => Err(e),
        }
    }

    pub fn resize(self, size: opencv::core::Size) -> Self {
        if opencv::core::Size::new(self.image_f.cols(), self.image_f.rows()) != size {
            let mut dst_image_f = Mat::default();
            let mut dst_beta_f = Mat::default();
            opencv::imgproc::resize(
                &self.image_f,
                &mut dst_image_f,
                size,
                0.,
                0.,
                opencv::imgproc::INTER_CUBIC,
            )
            .unwrap();
            opencv::imgproc::resize(
                &self.beta_f,
                &mut dst_beta_f,
                size,
                0.,
                0.,
                opencv::imgproc::INTER_CUBIC,
            )
            .unwrap();
            return Self {
                image_f: dst_image_f,
                beta_f: dst_beta_f,
            };
        }
        self
    }

    pub const fn beta(&self) -> &Mat {
        &self.beta_f
    }

    pub const fn rgb(&self) -> &Mat {
        &self.image_f
    }
}

#[test]
fn alpha_image_is_send() {
    fn assert<T: Send>() {}
    assert::<AlphaImage>();
}

#[instrument]
fn prep_alpha_blend(rgba: Mat) -> Result<(Mat, Mat)> {
    let (alpha_f32, rgb_f32) = {
        let mut split_planes = Vector::<Mat>::new();
        let mut alpha_planes = Vector::<Mat>::new();
        opencv::core::split(&rgba, &mut split_planes)?;
        alpha_planes.push(split_planes.get(3)?);
        alpha_planes.push(split_planes.get(3)?);
        alpha_planes.push(split_planes.get(3)?);
        split_planes.remove(3)?;
        let mut alpha = Mat::default();
        opencv::core::merge(&alpha_planes, &mut alpha)?;
        let mut rgb = Mat::default();
        opencv::core::merge(&split_planes, &mut rgb)?;

        let mut alpha_f32 = Mat::default();
        alpha.convert_to(&mut alpha_f32, CV_32F, 1. / 255., 0.)?;

        let mut rgb_f32 = Mat::default();
        rgb.assign_to(&mut rgb_f32, CV_32F)?;

        (alpha_f32, rgb_f32)
    };

    let mut rgb_f32_scaled = Mat::default();
    opencv::core::multiply(&rgb_f32, &alpha_f32, &mut rgb_f32_scaled, 1., -1)?;

    let mut alpha_f32_inv = Mat::default();
    opencv::core::subtract(
        &Scalar::all(1.),
        &alpha_f32,
        &mut alpha_f32_inv,
        &Mat::default(),
        -1,
    )?;

    Ok((alpha_f32_inv, rgb_f32_scaled))
}
