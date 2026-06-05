use std::env;
use std::path::PathBuf;

fn main() {
    let pothos_dir = "C:\\Program Files\\PothosSDR";

    // Link to libliquid.dll via the import library
    println!("cargo:rustc-link-search=native={pothos_dir}\\lib");
    println!("cargo:rustc-link-lib=dylib=libliquid");

    // Ensure the DLL can be found at runtime (PothosSDR\bin is on PATH via build.bat)
    println!("cargo:rustc-link-search=native={pothos_dir}\\bin");

    let header = format!("{pothos_dir}\\include\\liquid\\liquid.h");

    let bindings = bindgen::Builder::default()
        .header(&header)
        .clang_arg(format!("-I{pothos_dir}\\include"))
        // NCO
        .allowlist_function("nco_crcf_.*")
        .allowlist_type("nco_crcf")
        // Resampler (complex)
        .allowlist_function("msresamp_crcf_.*")
        .allowlist_type("msresamp_crcf")
        // Resampler (real-valued, for RDS audio path)
        .allowlist_function("msresamp_rrrf_.*")
        .allowlist_type("msresamp_rrrf")
        // FIR filter (complex)
        .allowlist_function("firfilt_crcf_.*")
        .allowlist_type("firfilt_crcf")
        // FIR filter (real-valued, for RDS subcarrier filtering)
        .allowlist_function("firfilt_rrrf_.*")
        .allowlist_type("firfilt_rrrf")
        // FM demod
        .allowlist_function("freqdem_.*")
        .allowlist_type("freqdem_s")
        // AM/SSB demod
        .allowlist_function("ampmodem_.*")
        .allowlist_type("ampmodem_s")
        .allowlist_type("liquid_ampmodem_type")
        // Kaiser filter design
        .allowlist_function("liquid_firdes_kaiser")
        // Derive Debug for structs
        .derive_debug(true)
        .derive_default(true)
        // Use core types
        .use_core()
        // Generate bindings
        .generate()
        .expect("Failed to generate LiquidDSP bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Failed to write bindings");
}
