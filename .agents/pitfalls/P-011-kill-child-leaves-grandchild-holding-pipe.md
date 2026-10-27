# P-011 kill 子进程后管道被孙进程握着

## 现象

`pi.execParallel` 的超时用例 `timo exec -c "sleep 30"` 到 200ms 就 kill 了外壳，`ExecOutput.code` 也确认为 `-1`，但整个测试仍耗时 29.4 秒才结束——kill 明明成功了，时间花在哪？

## 根因

输出管道有两个写端：子进程和它继承的孙进程。`child.kill()` 只终止外壳，`sleep 30` 仍然握着 stdout/stderr 写端，读端的 `read_to_end` 等不到 EOF；用 `JoinHandle::join()` 取读数就把"已经 kill"变成了"再等 30 秒"。管道写满时必须持续消费这一点又要求读线程先于等待启动，两个约束叠在一起才暴露。

## 修复

读线程改投递到 `std::sync::mpsc::Receiver<String>`，取数用 `recv_timeout(800ms)`：kill 路径最多等各管道 0.8 秒，按已读到的部分返回；同时在循环退出后补一次 `child.wait()` reap 直接子进程。超时用例 29.4 秒 → 1.8 秒。

## 预防

- "kill 了就应该立刻返回"这类断言必须写进测试（`elapsed < 5s` 与 `code == -1` 同时断言），否则实现可以慢慢等而测试照样绿。
- 凡是被 kill 的进程可能 spawn 过东西，管道读取一律要有界：等 EOF 等于等所有写端关闭，而我们控制不了孙进程。
- 异步读子进程输出用"线程 + channel + `recv_timeout`"，不要用 `JoinHandle::join()` 当同步取值手段。
