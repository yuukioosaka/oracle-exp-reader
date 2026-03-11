# oracle-exp-reader

> High-performance Oracle EXP dump file parser written in Rust

Oracle の旧来の `exp` ユーティリティで作成したダンプファイル（`.dmp`）をデータベース接続なしに直接解析するCLIツール／ライブラリです。

---

## 特徴

- **高速** — `memmap2` によるゼロコピー読み取りで 2〜5 GB/s のスループット
- **ゼロアロケーション走査** — `&[u8]` スライスで元バッファを直接参照
- **DB接続不要** — `.dmp` ファイル単体で解析可能
- **複数出力形式** — CSV / SQL INSERT / JSON Lines に対応
- **マルチ文字セット** — Shift_JIS・GBK・UTF-8・WE8MSWIN1252 など20種類対応
- **Oracle型デコード** — NUMBER・DATE・TIMESTAMP を人間が読める文字列に変換

---

## インストール

### 前提条件

- Rust 1.70 以上 ([rustup.rs](https://rustup.rs))
- Windows の場合: Visual C++ Build Tools または MinGW

```bash
git clone https://github.com/yourname/oracle-exp-reader.git
cd oracle-exp-reader
cargo build --release
```

ビルド成果物は `target/release/oracle-exp-reader`（Windows では `.exe`）に生成されます。

---

## 使い方

### ヘッダ情報の確認

```bash
oracle-exp-reader info dump.dmp
```

```
File          : dump.dmp
Size          : 524288 bytes (0.50 MB)
Export version: EXPORT:V11.02.00
Character set : JA16SJIS
Tables found  : 12
Total rows    : 48320
DDL statements: 37
```

### DDL の抽出

```bash
oracle-exp-reader ddl dump.dmp > schema.sql
```

```sql
CREATE TABLE SCOTT.EMP (
    EMPNO NUMBER(4) NOT NULL,
    ENAME VARCHAR2(10),
    ...
);
```

### テーブルデータのエクスポート

```bash
# CSV形式
oracle-exp-reader export dump.dmp -f csv

# JSON Lines形式
oracle-exp-reader export dump.dmp -f json

# SQL INSERT文
oracle-exp-reader export dump.dmp -f sql --batch 500

# 特定テーブルのみ（部分一致）
oracle-exp-reader export dump.dmp -f csv -t EMP

# 先頭1000行のみ
oracle-exp-reader export dump.dmp -f json --limit 1000
```

### レコード診断（バイナリ確認）

```bash
# レコード一覧
oracle-exp-reader records dump.dmp -n 50

# ヘックスダンプ付き
oracle-exp-reader records dump.dmp -n 10 --hex
```

```
    OFFSET                      TYPE      LENGTH  PREVIEW
----------------------------------------------------------------------
         0                FileHeader         512  EXPORT:V11.02.00....
       516              CharacterSet          10  JA16SJIS
       530          SessionParameters          8  ........
       542              DdlStatement         284  CREATE TABLE SCOTT.EMP
```

### バイナリの直接確認

```bash
oracle-exp-reader hexdump dump.dmp --offset 0x800 -n 256
```

---

## コマンドリファレンス

```
USAGE:
    oracle-exp-reader <COMMAND>

COMMANDS:
    info       ダンプファイルのヘッダ情報を表示
    records    全レコードのタイプ・オフセット一覧を表示
    ddl        DDL文（CREATE TABLE等）を抽出
    export     テーブルデータをエクスポート
    hexdump    ファイルの生バイトをhex表示
    help       ヘルプを表示

export オプション:
    -f, --format <FORMAT>  出力形式 [csv|sql|json] (default: csv)
    -t, --table <TABLE>    テーブル名フィルタ（部分一致）
        --limit <N>        最大行数 (0=無制限)
        --batch <N>        SQL形式のINSERT数/バッチ (default: 1000)
```

---

## ライブラリとして使う

`Cargo.toml` に追加:

```toml
[dependencies]
oracle-exp-reader = { path = "path/to/oracle-exp-reader" }
```

```rust
use oracle_exp_reader::DumpReader;
use oracle_exp_reader::reader::DumpEvent;

fn main() -> anyhow::Result<()> {
    let reader = DumpReader::open("dump.dmp")?;

    println!("Version : {}", reader.header().export_version);
    println!("Charset : {}", reader.header().charset);

    for event in reader.events() {
        match event? {
            DumpEvent::DdlStatement { sql, .. } => {
                println!("DDL: {}", &sql[..sql.len().min(80)]);
            }
            DumpEvent::Row { table, row_index, raw_values, .. } => {
                println!("[{}] row {}: {} cols", table, row_index, raw_values.len());
            }
            DumpEvent::TableEnd { table, rows_written } => {
                println!("Table {} done: {} rows", table, rows_written);
            }
            DumpEvent::EndOfFile => break,
            _ => {}
        }
    }

    Ok(())
}
```

---

## プロジェクト構成

```
src/
├── lib.rs       ライブラリルート・公開API
├── types.rs     レコード型・カラム型・データ構造
├── error.rs     エラー型 (thiserror)
├── parser.rs    コアバイナリパーサー（ゼロコピー）
├── reader.rs    高レベル DumpReader API
├── output.rs    CSV / SQL / JSON 出力フォーマッター
└── main.rs      CLI エントリポイント
```

---

## 対応する Oracle EXP フォーマット

| Oracle バージョン | 対応状況 |
|---|---|
| Oracle 8i | ✅ |
| Oracle 9i | ✅ |
| Oracle 10g | ✅ |
| Oracle 11g | ✅ |
| Oracle 12c 以降 | ⚠️ 未検証（expdp推奨） |

> **注意**: Oracle 10g 以降の `expdp`（Data Pump）形式には対応していません。

---

## 対応 Oracle データ型

| 型 | デコード結果 |
|---|---|
| VARCHAR2 / CHAR | 文字列（NLS変換あり） |
| NUMBER / FLOAT | 10進数文字列 |
| DATE | `YYYY-MM-DD HH:MM:SS` |
| TIMESTAMP | `YYYY-MM-DD HH:MM:SS.nnnnnnnnn` |
| RAW / BLOB | 16進数文字列 (`0x...`) |
| CLOB | テキスト |
| NULL | `NULL` |

---

## 性能の目安

| 処理内容 | スループット |
|---|---|
| レコード走査のみ | 2〜5 GB/s |
| DDL 抽出 | 1〜3 GB/s |
| CSV 出力 | 200〜800 MB/s |
| SQL INSERT 生成 | 100〜400 MB/s |

> リリースビルド（`--release`）はデバッグビルドの3〜10倍高速です。

```bash
cargo build --release
./target/release/oracle-exp-reader info dump.dmp
```

---

## 依存クレート

| クレート | 用途 |
|---|---|
| `memmap2` | メモリマップドファイル読み取り |
| `encoding_rs` | NLS文字セット変換 |
| `clap` | CLI引数パース |
| `thiserror` | エラー型定義 |
| `serde_json` | JSON Lines出力 |
| `anyhow` | エラーハンドリング |

---

## ライセンス

MIT License

---

## 関連ツール

- [Ora2Pg](https://github.com/darold/ora2pg) — Oracle → PostgreSQL マイグレーション
- Oracle `imp` — 純正インポートツール（`SHOW=Y` でDDL確認のみ可能）
