# P-002 edit 精确替换重复后续行

## 现象

真实任务把 `notes.txt`（2 行）替换一个词后，文件变成 5 行：多出空行、`owner: timo` 重复。模型自己发现输出异常并怀疑工具。

## 根因

`apply_to_file` 中 `splice_exact` 返回的是**整个文件**的行数组（prefix+block+suffix），却被 `replace_range(&mut new_lines, range, new_block)` 当作"该 range 的替换块"再 splice 一次——block 里已包含的后续行又被原数组尾部追加一遍。

## 修复

改为在规范化文本上计算字节区间（精确=命中片段，保留同行前后缀；模糊=整行块），按区间倒序 `replace_range`。删除 `splice_exact` / `replace_range`。提交 `2f725e3`。

## 预防

- 端到端文件测试必须断言**精确全文**（`assert_eq!`），不能只 `contains("keep")`——旧 CRLF 测试就是只查包含，才漏掉重复行。
- 新增写回类逻辑时同时覆盖：单行、行内片段、跨行、多编辑、CRLF+BOM 五种形态。
