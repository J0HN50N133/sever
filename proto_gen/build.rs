fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .out_dir("src/") // Output to proto_gen's src directory
        .compile_protos(&["../proto/revocation.proto"], &["../proto/"])?;
    Ok(())
}
