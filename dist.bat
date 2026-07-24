@echo off
REM icon.ico is automatically embedded in the .exe via build.rs (embed-resource crate)
cargo build --release && copy /Y target\release\retro-vis.exe .