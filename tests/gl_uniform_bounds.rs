#![cfg(not(target_arch = "wasm32"))]

mod support;

use miniquad::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use support::{calls, context, Call};

fn pipeline(context: &mut GlContext, uniforms: Vec<UniformDesc>) {
    let shader = context
        .new_shader(
            ShaderSource::Glsl {
                vertex: "vertex",
                fragment: "fragment",
            },
            ShaderMeta {
                images: vec![],
                uniforms: UniformBlockLayout { uniforms },
            },
        )
        .unwrap();
    let pipeline = context.new_pipeline(&[], &[], shader, PipelineParams::default());
    context.apply_pipeline(&pipeline);
}

fn rejects<const N: usize>(uniforms: Vec<UniformDesc>, data: &[f32; N]) {
    let (_guard, mut context) = context();
    pipeline(&mut context, uniforms);
    let result = catch_unwind(AssertUnwindSafe(|| {
        context.apply_uniforms(UniformsSource::table(data));
    }));
    assert!(result.is_err(), "undersized uniform data was accepted");
    assert!(
        calls().is_empty(),
        "uniform data reached OpenGL before validation"
    );
}

#[test]
fn rejects_short_matrix() {
    rejects(
        vec![UniformDesc::new("matrix", UniformType::Mat4)],
        &[0.0; 4],
    );
}

#[test]
fn rejects_short_array() {
    rejects(
        vec![UniformDesc::new("colors", UniformType::Float4).array(2)],
        &[0.0; 4],
    );
}

#[test]
fn validates_the_whole_block_before_uploading() {
    rejects(
        vec![
            UniformDesc::new("first", UniformType::Float4),
            UniformDesc::new("second", UniformType::Int4),
        ],
        &[0.0; 6],
    );
}

#[test]
fn includes_inactive_uniforms_in_the_layout() {
    rejects(
        vec![
            UniformDesc::new("inactive", UniformType::Mat4),
            UniformDesc::new("last", UniformType::Float4),
        ],
        &[0.0; 4],
    );
}

#[test]
fn uploads_valid_mixed_uniforms_at_byte_offsets() {
    let (_guard, mut context) = context();
    pipeline(
        &mut context,
        vec![
            UniformDesc::new("first", UniformType::Float3),
            UniformDesc::new("ints", UniformType::Int2).array(2),
            UniformDesc::new("matrix", UniformType::Mat4),
        ],
    );
    let data = [0.0f32; 23];
    context.apply_uniforms(UniformsSource::table(&data));
    let pointer = data.as_ptr() as usize;
    assert_eq!(
        calls(),
        vec![
            Call::Uniform {
                components: 3,
                count: 1,
                pointer
            },
            Call::Uniform {
                components: 2,
                count: 2,
                pointer: pointer + 12
            },
            Call::Uniform {
                components: 16,
                count: 1,
                pointer: pointer + 28
            },
        ]
    );
}
