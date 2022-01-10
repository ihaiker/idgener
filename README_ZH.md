# idgener

一个多模式、多数据集、分布式ID生成器。

## 特性

1. [x] 多模式
   - [ ] 顺序递增ID
   - [x] [snowflake模式](./docs/snowflake.md)
   - [x] [订单号模式](./docs/order.md)
2. [x] 多数数据集
   可根据用户配置在不同数据集上配置
3. 分布式 多节点无单点故障，无依赖项

## 架构

## 性能测试

[1]: https://stevenbai.top/rust/lock-freedom-without-garbage-collection/ '无锁实现'
