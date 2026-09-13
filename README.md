# SkyWallpaper

Windows 动态壁纸：天色随本地时间变化，天气来自 Open-Meteo。

需要 Rust 1.92+、Windows 10/11，以及本仓库旁的 [NanaUI](https://github.com/sena-nana/NanaUI)（默认 path `../NanaUI`）。

## 构建

```bash
cargo run --release
```

设置窗口用 NanaUI；壁纸层是独立 WGPU，画在桌面图标下面的 WorkerW。托盘可打开设置、暂停、退出。

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

预览窗 + 滑块面板。时间、季节、天气可调；关掉预览即结束。

## 行为

- 天色跟本机本地时钟：高度角和季节决定 6×4 色盘，铺成偏心 mesh（光井不居中，季节绕左右轨道、高度角改高低、时间慢漂）。不画太阳/月亮圆盘。夜空保持壁纸可读，不是压成黑屏
- 启动时用 IP 粗定位，设置里可搜索城市覆盖
- Open-Meteo 失败时回退晴天模拟，昼夜仍正确
- 插电约 30 FPS，电池约 15 FPS，全屏游戏时暂停
- 配置：`%APPDATA%\SkyWallpaper\config.toml`
- 开机自启写在当前用户 `Run` 键

不要和 Wallpaper Engine 等其它动态壁纸同时抢 WorkerW。部分精简 Shell 上挂接失败时会退到 `--preview` 窗口。

## 天空模型

画面是 Helios 式抽象 mesh：天顶 / 中层 / 地平色盘（高度角六相 × 四季，OKLab）铺成四块偏心椭圆。季节把光井放在左右轨道上（冬右夏左，春分秋分连续过渡），高度角改高低，时间做慢漂。没有地平色带或日盘。晨昏把暖色压进光井（通道偏移源自 Andrew Helmer [*Production Sky Rendering*](https://www.shadertoy.com/view/slSXRW) / [dnlzro/horizon](https://github.com/dnlzro/horizon)，MIT）。云的亮面跟着光井。夜空是分层 cell 星场，云雾会盖住星。天气用色调叠加：雨是全屏湿玻璃（圆头/残珠折射，水迹只刮开干区结雾；MIT 原创，不是 Shadertoy 移植），雪是多层近大远小软片，雾是地平指数体积。另有云间闪光。`cargo debug` 的 precip 滑块可从无雨扫到暴雨。

## 许可

MIT。天空模型部分同时遵循 Helmer / Horizon 的 MIT 署名要求。
