# SkyWallpaper

Windows 动态壁纸：物理天空随本地时间变化，天气来自 Open-Meteo。

需要 Rust 1.92+、Windows 10/11，以及本仓库旁的 [NanaUI](https://github.com/sena-nana/NanaUI)（默认 path `../NanaUI`）。

## 构建

```bash
cargo run -p skywallpaper --release
```

设置窗口用 NanaUI；壁纸层是独立 WGPU，画在桌面图标下面的 WorkerW。托盘可打开设置、暂停、退出。

## 开发预览

普通窗口验证天空与天气（不是产品设置里的预览）：

```bash
cargo run -p skywallpaper -- --preview
cargo run -p skywallpaper -- --preview --weather rain --lat 39.9 --lon 116.4
```

`--weather`：`clear` / `cloud` / `rain` / `snow` / `fog` / `thunder`。

## 行为

- 天色只跟本机本地时钟，按经纬度算太阳/月亮
- 启动时用 IP 粗定位，设置里可搜索城市覆盖
- Open-Meteo 失败时回退晴天模拟，昼夜仍正确
- 插电约 30 FPS，电池约 15 FPS，全屏游戏时暂停
- 配置：`%APPDATA%\SkyWallpaper\config.toml`
- 开机自启写在当前用户 `Run` 键

不要和 Wallpaper Engine 等其它动态壁纸同时抢 WorkerW。部分精简 Shell 上挂接失败时会退到 `--preview` 窗口。

## 天空模型

大气底色采用 Sébastien Hillaire, *A Scalable and Production Ready Sky and Atmosphere Rendering Technique*（2020）的介质系数与单次散射，实现源自 Andrew Helmer [*Production Sky Rendering*](https://www.shadertoy.com/view/slSXRW)（MIT），并由 [dnlzro/horizon](https://github.com/dnlzro/horizon) 的 `src/gradient.ts` 整理。本仓库把它搬进 2D WGSL（太阳仍走三维方位），不是网页那条 CSS 线性渐变。

## 许可

MIT。天空模型部分同时遵循 Helmer / Horizon 的 MIT 署名要求。
