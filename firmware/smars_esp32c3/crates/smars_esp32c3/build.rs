//! 构建脚本：转发 esp-idf-sys（links="esp_idf"）传播的构建元数据。
//!
//! esp-idf-template 的标准写法：链接参数（libnewlib、esp-idf 组件库、
//! `--ldproxy-linker` 标记等）经 `DEP_esp_idf_EMBUILD_LINK_ARGS` 传入，
//! 必须由应用 crate 转发才会作用到最终二进制链接。
//! 末尾附加兜底：若元数据缺失（如构建顺序异常），按已安装工具链注入
//! `--ldproxy-linker`，否则 ldproxy 0.3.5 会因缺该参数 panic。

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    embuild::espidf::sysenv::output();

    // 兜底：手工注入 --ldproxy-linker（正常情况下上面的转发已包含）
    let tools = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../.embuild/espressif/tools/riscv32-esp-elf");

    let mut gccs: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&tools) {
        for entry in entries.flatten() {
            let gcc = entry
                .path()
                .join("riscv32-esp-elf/bin/riscv32-esp-elf-gcc");
            if gcc.is_file() {
                gccs.push(gcc);
            }
        }
    }
    // 优先 ESP-IDF 5.3.2 要求的 esp-13.2.0_20240530，其余取字典序最后一个兜底
    gccs.sort();
    if let Some(gcc) = gccs
        .iter()
        .find(|p| p.to_string_lossy().contains("esp-13.2.0_20240530"))
        .or_else(|| gccs.last())
    {
        println!("cargo:rustc-link-arg-bins=--ldproxy-linker={}", gcc.display());
    }
}
