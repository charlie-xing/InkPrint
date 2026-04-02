# InkPrint

Turn your BOOX (or any Android e-ink reader) into a wireless network printer.  
Documents printed from any device on your LAN are saved as PDF directly to the BOOX.

---

## How it works

```
macOS / Windows / Linux / iOS / Android
        │  print via IPP over WiFi
        ▼
  InkPrint (Android app on BOOX)
  ├── IPP/HTTP server  (port 6310)
  ├── mDNS/Bonjour advertiser  (_ipp._tcp + AirPrint subtype)
  └── Saves PDF to  /Documents/InkPrint/
```

- **IPP 2.0** server implemented in Rust (`inkprint-core` crate)
- **AirPrint** compatible — macOS, iOS, and iPadOS discover it with zero configuration
- **Bonjour/mDNS** advertisement broadcasts both `_ipp._tcp` and `_universal._sub._ipp._tcp` so all major platforms can auto-discover the printer
- **Pure PDF** storage — every print job is saved as a PDF file, ideal for e-ink reading
- Rust core compiled to `aarch64-linux-android` via **cargo-ndk**, bridged to Kotlin via **UniFFI**

---

## Requirements

| Component | Version |
|-----------|---------|
| Android (target device) | 8.0+ (API 26+) |
| Android NDK | r29 |
| Rust toolchain | nightly |
| cargo-ndk | latest |
| JDK | 17 (Java 25 incompatible with Kotlin plugin) |
| macOS build host | Apple Silicon recommended |

---

## Build

```bash
# 1. Build Rust .so for Android arm64
make rust-build-android

# 2. Copy .so (UniFFI loads by namespace name)
cp android/app/src/main/jniLibs/arm64-v8a/libinkprint_core.so \
   android/app/src/main/jniLibs/arm64-v8a/libuniffi_inkprint.so

# 3. Build APK
cd android
JAVA_HOME=/opt/homebrew/opt/openjdk@17 ./gradlew assembleDebug

# 4. Install on device
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

> **Note:** Use the rustup nightly toolchain (`RUSTC=~/.rustup/toolchains/nightly-aarch64-apple-darwin/bin/rustc`).  
> Homebrew's `rustc` does not include Android targets.

---

## Usage

1. Install the APK on your BOOX device
2. Open InkPrint and tap **Start Printer Service**
3. Add the printer on your computer/phone (see below)
4. Print — the PDF appears in the app's file list and in `Documents/InkPrint/` on the BOOX

---

## Adding the printer

### macOS (AirPrint — recommended)

1. **System Settings → Printers & Scanners → Add Printer, Scanner or Fax…**
2. InkPrint appears in the list automatically
3. **Use: AirPrint** is selected — click **Add**

No driver download needed. Works on macOS Ventura, Sonoma, and Sequoia.

**Alternative — force PDF via terminal (if AirPrint sends PostScript):**

```bash
cat > /tmp/inkprint.ppd << 'EOF'
*PPD-Adobe: "4.3"
*FormatVersion: "4.3"
*LanguageVersion: English
*LanguageEncoding: ISOLatin1
*Manufacturer: "InkPrint"
*ModelName: "InkPrint"
*NickName: "InkPrint PDF Printer"
*PSVersion: "(3010.000) 0"
*LanguageLevel: "3"
*ColorDevice: False
*DefaultColorSpace: Gray
*cupsVersion: 2.2
*cupsFilter2: "application/vnd.cups-pdf application/pdf 0 -"
*DefaultPageSize: A4
*PageSize A4/A4: "<</PageSize[595 842]>>setpagedevice"
*PageSize Letter/Letter: "<</PageSize[612 792]>>setpagedevice"
*DefaultPaperDimension: A4
*PaperDimension A4/A4: "595 842"
*PaperDimension Letter/Letter: "612 792"
*DefaultImageableArea: A4
*ImageableArea A4/A4: "0 0 595 842"
*ImageableArea Letter/Letter: "0 0 612 792"
EOF

lpadmin -x InkPrint 2>/dev/null
lpadmin -p InkPrint -E \
  -v ipp://<BOOX_IP>:6310/ipp/print \
  -P /tmp/inkprint.ppd
```

---

### Windows 10 / 11

**Automatic:**
1. Settings → Bluetooth & devices → Printers & scanners
2. Click **Add device** — InkPrint appears automatically (requires Bonjour service)
3. Click **Add device** to confirm

**Manual (add by IP):**
1. Settings → Printers & scanners → Add device
2. "The printer that I want isn't listed" → Add a printer using an IP address or hostname
3. Protocol: **IPP** / Hostname: `<BOOX_IP>` / Port: `6310` / Queue: `ipp/print`
4. Driver: Generic / Text Only

---

### Linux

```bash
# Add printer (driverless, IPP Everywhere)
sudo lpadmin -p InkPrint -E \
  -v ipp://<BOOX_IP>:6310/ipp/print \
  -m everywhere

# Set as default (optional)
sudo lpoptions -d InkPrint

# Print
lp -d InkPrint /path/to/document.pdf
```

Works on Ubuntu, Debian, Fedora, Arch, and any distro with CUPS.

---

### iOS / iPadOS — AirPrint, zero config

1. Connect iPhone/iPad to the **same WiFi network** as the BOOX
2. In any app: **Share → Print**
3. Tap **Select Printer** — InkPrint appears automatically
4. Tap **Print**

> ⚠️ Personal Hotspot limitation: if your iPhone is sharing its hotspot, devices connected to it cannot discover the printer. Use a shared WiFi router.

---

### Android

**Built-in Print Service (Android 8+):**
1. Settings → Connected devices → Connection preferences → Printing
2. Default Print Service → Enable
3. InkPrint appears automatically on the same WiFi

**Manual:**
1. In Default Print Service, tap Add printer
2. Enter: `ipp://<BOOX_IP>:6310/ipp/print`

**Third-party apps:** Print & Share, HP Smart, or Mopria Print Service all support IPP.

---

## Project structure

```
inkprint/
├── inkprint-core/          # Rust library
│   ├── src/
│   │   ├── lib.rs          # UniFFI entry point, public API
│   │   ├── ipp/            # IPP protocol: parser, types, operations, printer state
│   │   ├── server/         # HTTP server (hyper 1.x) + TCP listener
│   │   └── mdns/           # mDNS advertiser (mdns-sd, pure Rust)
│   ├── inkprint.udl        # UniFFI interface definition
│   └── Cargo.toml
├── android/                # Android app
│   └── app/src/main/
│       ├── kotlin/com/inkprint/app/
│       │   ├── MainActivity.kt     # Compose UI
│       │   ├── PrinterService.kt   # Foreground service
│       │   ├── BootReceiver.kt     # Auto-start on boot
│       │   └── InkPrintLib.kt      # UniFFI wrapper
│       └── jniLibs/arm64-v8a/     # Compiled .so files
├── Makefile
└── README.md
```

---

## Architecture notes

- **Port 6310** — Android blocks ports below 1024 for non-system apps; 631 (standard IPP) is not usable
- **mDNS from Rust** — Android's `NsdManager` API (< API 33) cannot register `_universal._sub._ipp._tcp` subtypes required for AirPrint auto-selection. The Rust `mdns-sd` crate advertises both `_ipp._tcp` and `_universal._sub._ipp._tcp` correctly
- **PDF-only storage** — the `cupsFilter2` PPD directive and IPP `document-format-accepted` list guide clients to send PDF; raw PostScript is accepted and stored but not rendered
- **UniFFI bridge** — Rust callbacks (`PrintJobCallback`) are implemented in Kotlin and called from the IPP job handler when a file is fully received

---

## License

MIT
