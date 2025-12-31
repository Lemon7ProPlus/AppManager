# 🚀 Rust Service Manager

![License](https://img.shields.io/badge/license-MIT-blue.svg)
![Rust](https://img.shields.io/badge/language-Rust-orange.svg)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey)

A lightweight, web-based process manager written in **Rust**. It serves as a simple alternative to Supervisor or PM2, designed with a focus on Windows compatibility (handling background processes/hidden windows) while supporting other platforms.

It comes with a modern, embedded Web UI (built with Pico.css) to manage your services effortlessly.

> **Note:** Ideally suited for managing local development tools (like Syncthing, Aria2, Databases) or self-hosted services.

---

## ✨ Features

- **Web Dashboard**: Clean, dark-mode UI to view status, PID, and control services.
- **Process Management**: Start, Stop, and Restart processes with ease.
- **Robust Process Killing**:
  - Handles process trees (e.g., kills both the wrapper and worker processes for apps like *Syncthing*).
  - Cleans up orphan processes by name if PID tracking fails.
- **Windows Optimization**: Support for `CREATE_NO_WINDOW` flags to run console apps silently in the background.
- **Configuration**:
  - Simple `YAML` based configuration.
  - Hot-reloading of service lists.
  - Drag-and-drop sorting in UI.
- **Environment Control**: Custom environment variables and working directories per service.
- **Keep Alive**: Optional automatic restart for crashed services.
- **Integrate [Aria2NG](https://github.com/mayswind/AriaNg)**: Easy to use for Aria2

## 📸 Screenshots

![Dashboard Preview](https://github.com/Lemon7ProPlus/AppManager/blob/master/docs/dashboard.png)

## 🛠️ Installation & Build

### Prerequisites
- [Rust Toolchain](https://www.rust-lang.org/tools/install) installed.

### Build from Source

```bash
# Clone the repository
git clone https://github.com/your-username/rust-service-manager.git
cd rust-service-manager

# Build for release
cargo build --release
```
The binary will be located at target/release/service-manager.

## 🚀 Usage

1. Create a services.yaml file (see configuration below).
2. Run the application:
```bash
./service-manager.exe
```
3. Open your browser and visit: http://localhost:3000 (or your configured port).

## 📄 Configuration (services.yaml)
The application uses a YAML file to define services. You can also import YAML directly via the Web UI.
```yaml
# Global settings
listen: "127.0.0.1:3000"  # Web dashboard address
keep_alive: 10            # Check interval in seconds (0 to disable)

services:
  - id: "syncthing"
    name: "Syncthing"
    exec: "syncthing.exe"
    # Optional: Working directory
    working_dir: "D:\\Tools\\Syncthing"
    # Optional: Arguments
    args: 
      - "-no-browser"
      - "-no-restart"
    # Optional: Environment variables
    env:
      STTRACE: "all"
    # Optional: Web interface link (clickable in UI)
    url: "http://127.0.0.1:8384"
    # Optional: Windows specific settings
    windows:
      # 134217728 = 0x08000000 (CREATE_NO_WINDOW) - Hides the console
      # 16 = 0x00000010 (CREATE_NEW_CONSOLE)
      creation_flags: 134217728
    # Optional: Auto start when manager starts
    autorun: true
```

## 🏗️ Project Structure
- src/main.rs: Entry point and HTTP server setup.
- src/manager.rs: Core logic for process spawning, killing (process tree handling), and monitoring.
- src/service.rs: Configuration structs and serialization.
- Frontend: The HTML/JS is embedded into the binary (or served statically) providing a single-file executable experience.

## 🤝 Contributing
Contributions are welcome!
1. Fork the project.
2. Create your feature branch (git checkout -b feature/AmazingFeature).
3. Commit your changes (git commit -m 'Add some AmazingFeature').
4. Push to the branch (git push origin feature/AmazingFeature).
5. Open a Pull Request.

## 📝 License
Distributed under the MIT License. See LICENSE for more information.
