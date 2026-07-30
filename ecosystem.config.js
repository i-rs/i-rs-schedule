module.exports = {
  apps: [
    {
      name: "i-rs-schedule",
      script: "/Users/mankong/volumes/code/i-rs/i-rs-schedule/target/release/i-rs-schedule",
      args: "dashboard",
      cwd: "/Users/mankong/volumes/code/i-rs/i-rs-schedule",
      exec_interpreter: "none",
      exec_mode: "fork",
      env: { RUST_LOG: "info" },
      max_memory_restart: "500M",
      log_date_format: "YYYY-MM-DD HH:mm:ss Z",
      error_file: "./logs/claw-error.log",
      out_file: "./logs/claw-out.log",
      merge_logs: true,
      kill_timeout: 10000,
    },
  ],
};
