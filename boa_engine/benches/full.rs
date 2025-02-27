//! Benchmarks of the whole execution engine in Boa.

use boa_engine::{
    context::DefaultHooks, object::shape::SharedShape, optimizer::OptimizerOptions, realm::Realm,
    Context, Source,
};
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

#[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
#[cfg_attr(
    all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
    global_allocator
)]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

fn create_realm(c: &mut Criterion) {
    c.bench_function("Create Realm", move |b| {
        let root_shape = SharedShape::root();
        b.iter(|| Realm::create(&DefaultHooks, &root_shape))
    });
}

macro_rules! full_benchmarks {
    ($({$id:literal, $name:ident}),*) => {
        fn bench_parser(c: &mut Criterion) {
            $(
                {
                    static CODE: &str = include_str!(concat!("bench_scripts/", stringify!($name), ".js"));
                    let mut context = Context::default();

                    // Disable optimizations
                    context.set_optimizer_options(OptimizerOptions::empty());

                    c.bench_function(concat!($id, " (Parser)"), move |b| {
                        b.iter(|| context.parse_script(black_box(Source::from_bytes(CODE))))
                    });
                }
            )*
        }
        fn bench_compile(c: &mut Criterion) {
            $(
                {
                    static CODE: &str = include_str!(concat!("bench_scripts/", stringify!($name), ".js"));
                    let mut context = Context::default();

                    // Disable optimizations
                    context.set_optimizer_options(OptimizerOptions::empty());

                    let statement_list = context.parse_script(Source::from_bytes(CODE)).expect("parsing failed");
                    c.bench_function(concat!($id, " (Compiler)"), move |b| {
                        b.iter(|| {
                            context.compile_script(black_box(&statement_list))
                        })
                    });
                }
            )*
        }
        fn bench_execution(c: &mut Criterion) {
            $(
                {
                    static CODE: &str = include_str!(concat!("bench_scripts/", stringify!($name), ".js"));
                    let mut context = Context::default();

                    // Disable optimizations
                    context.set_optimizer_options(OptimizerOptions::empty());

                    let statement_list = context.parse_script(Source::from_bytes(CODE)).expect("parsing failed");
                    let code_block = context.compile_script(&statement_list).unwrap();
                    c.bench_function(concat!($id, " (Execution)"), move |b| {
                        b.iter(|| {
                            context.execute(black_box(code_block.clone())).unwrap()
                        })
                    });
                }
            )*
        }
    };
}

full_benchmarks!(
    {"Symbols", symbol_creation},
    {"For loop", for_loop},
    {"Fibonacci", fibonacci},
    {"Object Creation", object_creation},
    {"Static Object Property Access", object_prop_access_const},
    {"Dynamic Object Property Access", object_prop_access_dyn},
    {"RegExp Literal Creation", regexp_literal_creation},
    {"RegExp Creation", regexp_creation},
    {"RegExp Literal", regexp_literal},
    {"RegExp", regexp},
    {"Array access", array_access},
    {"Array creation", array_create},
    {"Array pop", array_pop},
    {"String concatenation", string_concat},
    {"String comparison", string_compare},
    {"String copy", string_copy},
    {"Number Object Access", number_object_access},
    {"Boolean Object Access", boolean_object_access},
    {"String Object Access", string_object_access},
    {"Arithmetic operations", arithmetic_operations},
    {"Clean js", clean_js},
    {"Mini js", mini_js}
);

criterion_group!(
    benches,
    create_realm,
    bench_parser,
    bench_compile,
    bench_execution,
);
criterion_main!(benches);
