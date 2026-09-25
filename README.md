# SkyWallpaper

Windows 动态壁纸：天色随本地时间变化，天气来自 Open-Meteo。

需要 Rust 1.92+、Windows 10/11，以及本仓库旁的 [NanaUI](https://github.com/sena-nana/NanaUI)（默认 path `../NanaUI`）。

## 构建

```bash
cargo run --release
```

`cargo run --release` 是默认产品启动命令；开发构建可使用 `cargo run`。

设置窗口用 NanaUI；壁纸层是独立 WGPU，画在桌面图标下面的 WorkerW。托盘可打开设置、暂停、退出。
正常启动不会打开普通预览窗口；如果 Explorer 的 WorkerW 暂时不可用，程序会在后台重试，宿主恢复后自动显示壁纸。

## 开发预览

普通窗口验证天空与天气（不是产品设置里的预览）：

```bash
cargo preview
cargo preview --weather rain --lat 39.9 --lon 116.4
```

`--weather`：`clear` / `cloud` / `rain` / `snow` / `fog` / `thunder`。

调运行时天空参数（不写配置）：

```bash
cargo debug
```

`cargo debug` 使用开发构建，在普通预览窗打开滑块面板；时间、季节、天气可调，关掉预览即结束。

## 性能基准

雨玻璃性能基准会测试 drizzle / light / storm 在 1080p 和 4K 下的 CPU
提交等待时间，并在适配器支持 timestamp query 时记录 GPU 时间：

```bash
cargo perf
```

`cargo perf` 使用 release 构建运行 `sky-gpu` 的 `rain_perf`，不会启动桌面壁纸。

CSV 中的 `cpu_wait_*` 包含编码、提交及等待 GPU 完成的时间；`gpu_*` 是命令编码器
首尾 timestamp 的设备执行时间。不支持 timestamp query 时，GPU 列为 `NaN`。

## 行为

- 天色跟本机本地时钟：高度角和季节决定 6×4 色盘，铺成偏心 mesh（光井不居中，季节绕左右轨道、高度角改高低、时间慢漂）。不画太阳/月亮圆盘。夜空保持壁纸可读，不是压成黑屏
- 启动时用 IP 粗定位，设置里可搜索城市覆盖
- Open-Meteo 失败时回退晴天模拟，昼夜仍正确
- 插电约 30 FPS，电池约 15 FPS，全屏游戏时暂停
- 配置：`%APPDATA%\SkyWallpaper\config.toml`
- 开机自启写在当前用户 `Run` 键

不要和 Wallpaper Engine 等其它动态壁纸同时抢 WorkerW。部分精简 Shell 上挂接失败时会退到 `--preview` 窗口。

## 天空模型

画面是 Helios 式抽象 mesh：天顶 / 中层 / 地平色盘（高度角六相 × 四季，OKLab）铺成四块偏心椭圆。季节把光井放在左右轨道上（冬右夏左，春分秋分连续过渡），高度角改高低，时间做慢漂。没有地平色带或日盘。晨昏把暖色压进光井（通道偏移源自 Andrew Helmer [*Production Sky Rendering*](https://www.shadertoy.com/view/slSXRW) / [dnlzro/horizon](https://github.com/dnlzro/horizon)，MIT）。云的亮面跟着光井。夜空是分层 cell 星场，云雾会盖住星。天气用色调叠加：雨使用 Martijn Steinrucken（BigWings）的 Heartfelt 雨玻璃算法（CC BY-NC-SA 3.0），包括原版 `StaticDrops`、`DropLayer2`、`Drops`、法线扰动和 LOD 模糊；雪是多层近大远小软片，雾是地平指数体积。另有云间闪光。`cargo debug` 的 precip 滑块可从无雨扫到暴雨。

## 许可

除 `crates/sky-gpu/src/glass.wgsl` 按 CC BY-NC-SA 3.0 提供外，其余代码使用
MIT。天空模型部分同时遵循 Helmer / Horizon 的 MIT 署名要求。
