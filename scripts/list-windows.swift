// 列出属于某个应用的窗口： 归谁 / 在不在屏上 / 哪一层 / 多大。
//
// 为什么需要它：这台机器的屏幕经常是锁着的，截不了图（screencapture 报
// "could not create image from display"），AX 也看不见应用窗口（票 09 记过）。
// 但**窗口元数据**不需要任何权限——CGWindowList 在这件事上要的是窗口列表，
// 不是像素内容。于是"面板到底弹没弹"这种问题，在锁屏的机器上也能验：
// onscreen=false 就是没弹。
//
// 用法：
//   swift scripts/list-windows.swift [过滤词]
//
// 过滤词是应用名里的一段（不区分大小写），默认找 "focus"。每个窗口一行：
//   owner=FocusTimer onscreen=false layer=0 size=320x380

import CoreGraphics
import Foundation

let needle = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "focus"

guard let windows = CGWindowListCopyWindowInfo([.optionAll], kCGNullWindowID) as? [[String: Any]] else {
    FileHandle.standardError.write("列不出窗口\n".data(using: .utf8)!)
    exit(1)
}

var found = 0
for window in windows {
    let owner = window[kCGWindowOwnerName as String] as? String ?? "?"
    guard owner.lowercased().contains(needle) else { continue }

    let onscreen = (window[kCGWindowIsOnscreen as String] as? Bool) ?? false
    let layer = (window[kCGWindowLayer as String] as? Int) ?? -1
    let bounds = window[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let width = bounds["Width"] ?? 0
    let height = bounds["Height"] ?? 0

    print("owner=\(owner) onscreen=\(onscreen) layer=\(layer) size=\(width)x\(height)")
    found += 1
}
print("—— 共 \(found) 个属于「\(needle)」的窗口")
