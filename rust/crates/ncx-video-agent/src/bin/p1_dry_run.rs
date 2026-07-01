use std::process::ExitCode;

use ncx_video_agent::run_local_p1_dry_run;

fn main() -> ExitCode {
    let out_dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap().join("p1_dry_run_out"));

    match run_local_p1_dry_run(&out_dir) {
        Ok(output) => {
            println!("dry-run output: {}", out_dir.display());
            println!("db: {}", output.db_path.display());
            println!("rough_cut: {}", output.rough_cut_path.display());
            println!("failed_shots: {}", output.failed_shots_path.display());
            println!("manifest: {}", output.assembly_manifest_path.display());
            println!("trace: {}", output.trace_path.display());
            for path in &output.shot_trace_paths {
                println!("shot_trace: {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("p1 dry-run failed: {err}");
            ExitCode::FAILURE
        }
    }
}
