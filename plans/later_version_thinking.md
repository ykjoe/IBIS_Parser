# 往后版本思路

## ibis2ibstoml

### ibis协议分析思路
前端 & 后端目前一个使用pest，一个使用ibis_struct.rs，这样在ibis协议版本演进后，需要人工修改两个文件，风险较大。如果我们采用**Schema模式**，是否更适合这个工程？

以下是deepseek的说法：
```plaintext
Schema / IDL 代码生成（Schema-Driven CodeGen）方法
工业参考：Protobuf、FlatBuffers、OpenAPI、Thrift。
实现方式：
编写一份独立的元数据描述文件（如 ibis_schema.toml 或 JSON Schema）。
在 Rust 的 build.rs 阶段，解析该 Schema 并自动生成 frontend/ibis.pest 语法文件以及 backend/ibis_structure.rs 结构体代码。
缺点：引入了额外的构建期工具链（Code Generator），对纯 Rust 库来说显得过于沉重。
```
