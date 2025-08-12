# Timelapse Service (Rust)

Timelapse Service is a Rust-based web API that generates timelapse videos and ZIP archives from folders of timestamped JPEG images using FFmpeg. The service provides REST endpoints for different time ranges (24h, 48h, 1 week, specific days, custom ranges) and includes a web UI for browsing available folders.

Always reference these instructions first and fallback to search or bash commands only when you encounter unexpected information that does not match the info here.

## Working Effectively

- **CRITICAL: NEVER CANCEL long-running commands.** Build and test operations can take several minutes.

### Bootstrap and Build
- Install system dependencies: `sudo apt update && sudo apt install -y ffmpeg pkg-config libssl-dev`
- Build the project: `cargo build --release` -- takes 1m 45s. NEVER CANCEL. Set timeout to 3+ minutes.
- Quick check: `cargo check` -- takes 3m 5s on first run (with dependency downloads). NEVER CANCEL. Set timeout to 5+ minutes.
- Debug build: `cargo build` -- faster than release build but produces unoptimized binary.

### Testing
- Run tests: `cargo test` -- takes 56s fresh build, 43s incremental. NEVER CANCEL. Set timeout to 2+ minutes.
- The project has 2 unit tests that validate HTTP range request handling and caching headers.

### Running the Application
- **ALWAYS install system dependencies first:** `sudo apt update && sudo apt install -y ffmpeg`
- **Required environment variables:**
  - `OUTPUT_FOLDER`: Path to directory containing timestamped image folders (required)
  - `PORT`: Port number (optional, defaults to 8102)
- Run debug version: `OUTPUT_FOLDER=/path/to/images PORT=8102 cargo run`
- Run release version: `OUTPUT_FOLDER=/path/to/images PORT=8102 ./target/release/timelapse-service-rs`
- **NEVER CANCEL** - application startup includes ~4s compilation for debug builds.

### Docker Build
- **WARNING:** Docker builds may fail in some environments due to SSL certificate issues with crates.io.
- If Docker build fails with SSL errors, document this limitation.
- Expected Docker build time: Unknown (validation failed due to SSL issues).
- Docker image runs on port 8102 by default and requires OUTPUT_FOLDER mount.

## Validation Scenarios

**ALWAYS test functionality after making changes by running through these scenarios:**

### Basic Functionality Test
1. Install dependencies: `sudo apt install -y ffmpeg imagemagick` 
2. Create test image folder with timestamped JPEGs:
   ```bash
   mkdir -p /tmp/test-images/testfolder
   cd /tmp/test-images/testfolder
   NOW=$(date +%s)
   for i in {0..9}; do
     TIMESTAMP=$((NOW - i * 3600))
     convert -size 640x480 "xc:rgb($((255 - i * 10)),$((i * 10)),128)" "${TIMESTAMP}.jpg"
   done
   ```
3. Run application: `OUTPUT_FOLDER=/tmp/test-images PORT=8102 cargo run`
4. Test web UI: `curl http://localhost:8102/timelapse/` (should show HTML with testfolder)
5. Test healthcheck: `curl http://localhost:8102/healthcheck` (should return "OK")
6. **Test video generation:** `curl http://localhost:8102/timelapse/24/testfolder` (should return MP4 data)
7. **Test ZIP generation:** `curl "http://localhost:8102/timelapse/24/testfolder?format=zip"` (should return ZIP archive)
8. Check application logs for successful video creation messages

### Build Validation
- Always run `cargo test` before finalizing changes
- **ALWAYS** check for compilation warnings - address deprecation warnings when possible
- Run `cargo check` to validate syntax without full compilation
- For release builds, use `cargo build --release` (takes 1m 45s)

## Build Timing Expectations

- **`cargo check` (initial):** 3m 5s - NEVER CANCEL, set timeout to 5+ minutes
- **`cargo test` (fresh build):** 56s - NEVER CANCEL, set timeout to 2+ minutes
- **`cargo test` (incremental):** 43s - NEVER CANCEL, set timeout to 2+ minutes  
- **`cargo build --release`:** 1m 45s - NEVER CANCEL, set timeout to 3+ minutes
- **`cargo run` (debug, incremental):** ~4s compilation
- **Docker build:** FAILS due to SSL certificate issues in some environments

## Common Tasks

### Repository Structure
```
/home/runner/work/timelapse-service-rs/timelapse-service-rs/
├── Cargo.toml          # Dependencies and project config
├── Cargo.lock          # Dependency lock file
├── src/main.rs         # Main application code (~689 lines)
├── Dockerfile          # Multi-stage Docker build
└── .github/workflows/docker-publish.yml  # CI/CD pipeline
```

### Key Application Details
- **Port:** 8102 (default, configurable via PORT env var)
- **Image Requirements:** JPEG files named with Unix timestamps (e.g., `1755040282.jpg`)
- **Supported Endpoints:**
  - `GET /timelapse/24/:folder` - 24 hour timelapse
  - `GET /timelapse/48/:folder` - 48 hour timelapse  
  - `GET /timelapse/1w/:folder` - 1 week timelapse
  - `GET /timelapse/day/YYYY-MM-DD/:folder` - Specific day
  - `GET /timelapse/from/[ISO8601]/to/[ISO8601]/:folder` - Custom range
  - `GET /timelapse/` - Web UI index
  - `GET /healthcheck` - Health status
- **Query Parameters:**
  - `fps`: Frames per second (default: 20)
  - `format`: Output format (`zip` for ZIP archive, default: MP4 video)
  - `ffmpeg_args`: Custom FFmpeg arguments (comma-separated)
- **Caching:** Videos are cached in memory (10 item LRU cache) with 15-minute HTTP cache headers

### Dependencies
- **System:** ffmpeg (required for video processing), pkg-config, libssl-dev
- **Rust:** 1.88+ (current toolchain works)
- **Key Crates:** poem (web framework), tokio (async runtime), chrono (timestamps), color-eyre (error handling)

## Troubleshooting

### Build Issues
- If `cargo build` fails with SSL errors, ensure system certificates are updated
- Missing system packages cause linking errors - install pkg-config and libssl-dev
- Deprecated dependency warnings are expected (bitflags v0.7.0) - not critical

### Runtime Issues  
- **"OUTPUT_FOLDER env var required"** - Set OUTPUT_FOLDER environment variable
- **FFmpeg errors** - Ensure FFmpeg is installed and in PATH
- **Empty video responses** - Check image folder contains properly named timestamp files
- **Port binding errors** - Ensure port 8102 is available or specify different PORT

### Docker Issues
- SSL certificate errors during build are known limitation in some environments
- If Docker build fails, run natively using cargo instead
- Mount OUTPUT_FOLDER as volume when running containerized version

## Testing Notes

- Unit tests focus on HTTP range request handling and caching headers
- Manual validation requires actual image files and FFmpeg functionality
- Test image creation requires ImageMagick (`sudo apt install imagemagick`)
- Always verify actual video/ZIP generation works, not just HTTP responses

## CI/CD Pipeline

- GitHub Actions workflow builds and publishes Docker images
- Workflow triggers on pushes to master and version tags
- Uses multi-stage Docker build with Rust 1.85 and Debian Bookworm
- **Note:** CI may fail if SSL certificate issues affect Docker builds