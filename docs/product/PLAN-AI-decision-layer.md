# AI 决策层实施计划（1.1）

> 原则：AI/规则**影响决策**（结论、筛选、预选），不是角落里的解释按钮。  
> 仍：只解释不删除；默认可离线（规则）；有模型时自动升级。

## P0 — 已落地

| 项 | 说明 |
|---|---|
| 清理结论卡 | `CleanupConclusion` 扫描完成自动出现：空间 / 可清 / 保留 / 最大风险 + 三类操作 |
| 本地规则默认 | 未配置模型时标签「本地规则」；已配置自动「AI 解读」 |
| Copilot 主界面 | 软件列表上方常驻「想清理什么？」；无模型走 `ruleParseFilter` |
| 自动解读 | 扫描完成且 `aiEnabled` 时后台 `aiExplain` |

## P1 — 已落地

| 项 | 说明 |
|---|---|
| 三类联动 | 清理建议项预选 `defaultSelectable`；需确认 / 为何保留过滤 `riskFilter` |
| 风险标签 | 共享 / 用户数据 / 高风险在结论与行内 badge + 具体保留原因 |
| AI 行高亮 | 有 `aiNotes` 的行 accent 边条 + 底色 |

## P2 — 已落地

| 项 | 说明 |
|---|---|
| 清理前叙事 | 结论卡（为什么建议删/留） |
| 确认压缩 | 确认框 ≤2 条关键风险 + 备份说明（`useCleanupHandlers`） |
| 清理后叙事 | `ReportPanel` 固定「下一步」：成功 / 失败 / 跳过分流 |

## 文件

- `src/components/CleanupConclusion.tsx`
- `src/components/CopilotPanel.tsx`（+ `ruleParseFilter`）
- `src/components/ScanLeftoversView.tsx`（conclusion 插槽、riskFilter、AI 高亮）
- `src/components/ReportPanel.tsx`（下一步）
- `src/App.tsx` 接线
- `src/__tests__/copilotRule.test.ts`

## 验证

```powershell
npm test
npm run build
```
