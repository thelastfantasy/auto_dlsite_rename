# auto_dlsite_rename
自动重命名dlsite文件如(RJ123123.zip或RJ456456.rar)为[作者][RJ123123] 作品标题.zip，[作者][RJ456456] 作品标题.zip
目前只支持RJ开头的作品ID

# 准备工作

1. 编译 cargo build --release
2. 创建 settings.json文件，内容格式如下，出错的话检查：双引号，英文逗号，双反斜杠
```json
[
  "d:\\支持中文路径\\下载\\小动画\\survive\\",
  "d:\\支持中文路径\\下载\\小动画\\survive more\\",
]
```
