# Screen Reminder

一个使用 Rust + Slint 开发的屏幕提醒小项目。到达配置的定时时间后，会弹出一个置顶提醒窗口。

## 功能

- 使用 Slint 编写 UI：`ui/reminder.slint`
- 使用 Rust 编写定时与业务逻辑：`src/main.rs`
- 支持每天固定时间提醒
- 支持每隔 N 分钟提醒
- 支持每条提醒单独设置生效时间段，例如 09:00 到 22:00
- 支持每条提醒单独设置生效星期范围，例如周一到周五
- 弹窗置顶显示
- 支持“我知道了”和“5 分钟后再提醒”
- 可配置当前用户开机自动启动
- 可配置按 Esc 立即关闭弹窗，Esc 只隐藏提醒窗口，不退出程序
- 可配置普通置顶窗口提醒或全屏提醒
- 启动后常驻 Windows 系统托盘，可从托盘显示测试提醒或退出程序

## 运行

```bash
cargo run
```

第一次运行需要下载和编译依赖，时间会比较久。

程序启动后会常驻 Windows 屏幕右下角系统托盘：

- 左键点击托盘图标：显示测试提醒
- 右键点击托盘图标：打开菜单
- 菜单“显示测试提醒”：显示测试提醒
- 菜单“退出”：真正退出程序

提醒窗口中按 `Esc` 只会关闭/隐藏当前提醒窗口，不会退出后台程序。

## 开发测试：立即模式

开发时可以让程序启动后立刻弹出提醒，方便测试 UI 和置顶效果。

方式一：命令行参数

```bash
cargo run -- --dev-immediate
```

方式二：环境变量

```bash
SCREEN_REMINDER_DEV_IMMEDIATE=1 cargo run
```

立即模式只是在启动时把 `reminders.toml` 中的第一条提醒立即触发一次，之后正常定时逻辑仍然继续运行。

## 配置提醒

编辑项目根目录的 `reminders.toml`。

### 全局设置

```toml
[settings]
# true：程序启动时自动写入当前用户的开机启动项；false：自动移除开机启动项
start_on_boot = false
# true：提醒窗口弹出后，按 Esc 立即关闭窗口；false：Esc 不关闭
close_on_escape = true
# true：全屏提醒；false：普通置顶窗口提醒
fullscreen_reminder = false
# 渲染器：software 更稳定省内存；femtovg 使用 OpenGL
renderer = "software"
```

说明：

- `start_on_boot = true` 会在程序启动时写入 Windows 当前用户的开机启动注册表项。
- `start_on_boot = false` 会在程序启动时移除这个开机启动项。
- `fullscreen_reminder = true` 会使用全屏置顶提醒；`false` 使用普通置顶窗口。
- `renderer = "software"` 更稳定、省内存，不依赖 OpenGL 驱动；`renderer = "femtovg"` 使用 OpenGL 硬件加速。
- 开机启动使用的是当前运行的 exe 路径；建议先 `cargo build --release`，再从 `target/release/screen_reminder.exe` 启动一次来写入正式路径。

### 每天固定时间

```toml
[[reminders]]
title = "喝水提醒"
message = "起来活动一下，喝一杯水。"
at = "09:30"
```

### 每隔 N 分钟

```toml
[[reminders]]
title = "休息眼睛"
message = "看看远处，放松眼睛 1 分钟。"
every_minutes = 45
```

### 设置提醒生效时间段和星期范围

每条提醒都可以单独配置：

```toml
[[reminders]]
title = "喝水提醒"
message = "起来活动一下，喝一杯水。"
every_minutes = 20

# 仅在每天 09:00 到 22:00 之间生效，不会夜里提醒
active_start = "09:00"
active_end = "22:00"

# 仅周一到周五生效
weekday_start = "mon"
weekday_end = "fri"
```

说明：

- `active_start` / `active_end` 不填表示全天生效。
- `weekday_start` / `weekday_end` 不填表示全周生效。
- 星期支持：`mon`、`tue`、`wed`、`thu`、`fri`、`sat`、`sun`。
- 也支持中文：`周一`、`周二`、`周三`、`周四`、`周五`、`周六`、`周日`。
- 时间段支持跨午夜，例如 `active_start = "22:00"`、`active_end = "06:00"`。
- 星期范围支持跨周，例如 `weekday_start = "fri"`、`weekday_end = "mon"`。

## 打包构建

```bash
cargo build --release
```

生成的程序在：

```text
target/release/screen_reminder.exe
```

> 配置文件查找顺序：当前运行目录、exe 所在目录、项目源码目录。都找不到时，程序会在当前运行目录自动创建默认 `reminders.toml`。

## 文件结构

```text
Cargo.toml
build.rs
reminders.toml
assets/tray-icon.png
src/main.rs
ui/reminder.slint
ui/tray.slint
```
