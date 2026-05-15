#!/usr/bin/env python3
"""R1 写放大基准:模拟 entry_delivery 表在典型/重度/极端负载下的 INSERT OR REPLACE 吞吐与延迟。

与生产对齐:
- journal_mode = WAL
- busy_timeout = 5000ms
- foreign_keys = ON
- 文件落盘(非 :memory:),触发真实 fsync

场景:
- 普通用户:N=3 peer × M=100 entry  (300 行总写入)
- 重度用户:N=5 peer × M=500 entry  (2500 行)
- 极端用户:N=10 peer × M=1000 entry (10000 行)

每条 entry 模拟:
- 1 次 entry INSERT
- N 次 delivery INSERT OR REPLACE(对应 N 个 peer 的 dispatch 完成时落盘)
- 每条 entry 一个事务(贴近真实:LocalCapture + dispatch JoinSet 完成是一个语义动作)

不模拟:
- diesel/r2d2 池子开销(纯 SQLite raw capacity)
- 真实 tokio spawn_blocking 切换开销
- 真实 dispatch 完成的时间分布(我们假设 instant 落盘)
"""

import os
import sqlite3
import statistics
import tempfile
import time

SCHEMA = """
CREATE TABLE clipboard_entry (
    entry_id TEXT PRIMARY KEY,
    event_id TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    delivery_tracked INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE clipboard_entry_delivery (
    entry_id TEXT NOT NULL,
    target_device_id TEXT NOT NULL,
    status TEXT NOT NULL,
    reason_detail TEXT,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (entry_id, target_device_id),
    FOREIGN KEY (entry_id) REFERENCES clipboard_entry(entry_id) ON DELETE CASCADE
);

CREATE INDEX idx_entry_delivery_entry ON clipboard_entry_delivery(entry_id);
"""


def setup_db(path):
    conn = sqlite3.connect(path)
    # 与生产 pool.rs 对齐
    conn.execute("PRAGMA journal_mode = WAL")
    conn.execute("PRAGMA busy_timeout = 5000")
    conn.execute("PRAGMA foreign_keys = ON")
    conn.executescript(SCHEMA)
    conn.commit()
    return conn


def run_scenario(name, n_peers, m_entries):
    with tempfile.TemporaryDirectory() as td:
        db_path = os.path.join(td, "bench.db")
        conn = setup_db(db_path)
        cur = conn.cursor()

        per_entry_latencies_us = []
        peers = [f"peer_{i:03d}" for i in range(n_peers)]
        now_ms = int(time.time() * 1000)

        total_start = time.perf_counter()
        for i in range(m_entries):
            entry_id = f"entry_{i:06d}"
            event_id = f"event_{i:06d}"

            t0 = time.perf_counter()
            cur.execute(
                "INSERT INTO clipboard_entry "
                "(entry_id, event_id, created_at_ms, delivery_tracked) VALUES (?, ?, ?, 1)",
                (entry_id, event_id, now_ms),
            )
            # 模拟 N 个 dispatch 完成顺次落盘(JoinSet 是并发,但 SQLite 串行化写)
            for peer in peers:
                cur.execute(
                    "INSERT OR REPLACE INTO clipboard_entry_delivery "
                    "(entry_id, target_device_id, status, updated_at_ms) VALUES (?, ?, 'delivered', ?)",
                    (entry_id, peer, now_ms),
                )
            conn.commit()
            t1 = time.perf_counter()
            per_entry_latencies_us.append((t1 - t0) * 1_000_000)

        total_elapsed_s = time.perf_counter() - total_start
        conn.close()

        total_rows = m_entries * (1 + n_peers)
        throughput_rows_per_s = total_rows / total_elapsed_s

        p50 = statistics.median(per_entry_latencies_us)
        p99 = statistics.quantiles(per_entry_latencies_us, n=100)[98]
        mean = statistics.mean(per_entry_latencies_us)

        print(f"\n=== 场景: {name} (N={n_peers} peer, M={m_entries} entry) ===")
        print(f"  总写入行数:        {total_rows:>8} 行 (1 entry + {n_peers} delivery 每条)")
        print(f"  总耗时:            {total_elapsed_s*1000:>8.1f} ms")
        print(f"  吞吐(每秒行数):     {throughput_rows_per_s:>8.0f} rows/s")
        print(f"  每条 entry 延迟:")
        print(f"    p50:             {p50:>8.1f} μs")
        print(f"    mean:            {mean:>8.1f} μs")
        print(f"    p99:             {p99:>8.1f} μs")


def main():
    print("=" * 70)
    print("R1 · entry_delivery 写放大基准")
    print("=" * 70)
    print(f"Python sqlite3 {sqlite3.sqlite_version} (与生产 SQLite 同一引擎)")
    print(f"配置: WAL + busy_timeout=5000 + foreign_keys=ON")
    print(f"模式: 文件落盘(触发真实 fsync)")

    # 三组场景
    run_scenario("普通用户", n_peers=3, m_entries=100)
    run_scenario("重度用户", n_peers=5, m_entries=500)
    run_scenario("极端用户", n_peers=10, m_entries=1000)

    # 极端压力:模拟"机器人级"复制频率
    run_scenario("压力测试", n_peers=10, m_entries=5000)


if __name__ == "__main__":
    main()
