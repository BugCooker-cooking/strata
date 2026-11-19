#!/usr/bin/env python3
"""Audit real editing commands against feature mappings and native references.

Development checks reject broken links and stale reports. --require-p0 checks
the v1.0 feature scope. --require-complete checks the entire inventory.
"""
import argparse
import hashlib
import json
import re
import runpy
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MAPPING = ROOT / 'docs/research/ai-editing-entrypoints.json'
STATES = ROOT / 'docs/research/photoshop-feature-status.json'
MARKDOWN = ROOT / 'docs/plans/ai-editing-entrypoint-coverage.md'
REPORT = ROOT / 'docs/research/data/ai-editing-entrypoint-audit.json'
META = {'browse_catalog', 'search_catalog', 'describe_commands', 'invoke_command',
        'invoke_batch', 'inspect_document', 'view_canvas', 'ask_user'}
PATTERN = re.compile(r'"((?:system|document|file|edit|image|layer|select|adjust|retouch|paint|transform|filter|type|path|color|view)\.[a-z][a-z0-9_.]+)"')


def nodes(root, node, path, seen=None):
    if not isinstance(node, dict):
        return
    seen = set() if seen is None else seen
    key = (id(node), tuple(path))
    if key in seen:
        return
    seen = seen | {key}
    if '$ref' in node:
        ref = node['$ref']
        if not ref.startswith('#/'):
            raise ValueError(f'External schema reference: {ref}')
        value = root
        for part in ref[2:].split('/'):
            value = value[part.replace('~1', '/').replace('~0', '~')]
        yield from nodes(root, value, path, seen)
    for kind in ['oneOf', 'anyOf', 'allOf']:
        for branch in node.get(kind, []):
            yield from nodes(root, branch, path, seen)
    if path:
        child = node.get('properties', {}).get(path[0])
        if child is not None:
            yield from nodes(root, child, path[1:], seen)
    else:
        yield node


def audit(catalog):
    rows = list(runpy.run_path(str(ROOT / 'scripts/feature-coverage.py'))['inventory']())
    states = json.loads(STATES.read_text())
    manifest = json.loads(MAPPING.read_text())
    mappings = manifest['features']
    specs = {item['id']: item for item in catalog}
    if len(specs) != len(catalog):
        raise ValueError('Duplicate registered command IDs')
    unknown = set(mappings) - {row['id'] for row in rows}
    if unknown:
        raise ValueError(f'Mapping references unknown features: {sorted(unknown)}')
    for identity, entry in mappings.items():
        for command in entry['commands']:
            if command not in specs:
                raise ValueError(f'{identity}: unimplemented command {command}')
        if not set(entry.get('meta_tools', [])) <= META:
            raise ValueError(f'{identity}: unknown meta-tool')
        if entry['scope'] == 'editing' and (not entry['commands'] or not entry['prompts']):
            raise ValueError(f'{identity}: editing mapping needs commands and prompts')
        if entry['scope'] != 'editing' and not entry['note']:
            raise ValueError(f'{identity}: host/infrastructure scope needs a reason')
        for prompt in entry['prompts']:
            if not prompt['text'] or not prompt['query'] or prompt['command'] not in entry['commands']:
                raise ValueError(f'{identity}: incomplete prompt example')
        for variant in entry.get('variants', []):
            schema = specs[variant['command']]['params']
            accepted = []
            for node in nodes(schema, schema, variant['path'].split('.')):
                accepted.extend(node.get('enum', []))
                if 'const' in node:
                    accepted.append(node['const'])
            if not all(value in accepted for value in variant['values']):
                raise ValueError(f'{identity}: unsupported schema variant {variant}')
    native = {}
    for path in sorted((ROOT / 'apps/macos/Strata').rglob('*.swift')):
        for line, value in enumerate(path.read_text().splitlines(), 1):
            for match in PATTERN.finditer(value):
                native.setdefault(match[1], []).append(f'{path.relative_to(ROOT)}:{line}')
    # Native routers sometimes compare a namespace prefix instead of invoking a
    # command. Retain that evidence and require at least one real implementation
    # under it; a typo or an entirely future namespace still fails this gate.
    native_prefixes = {name: locations for name, locations in native.items() if name.endswith('.')}
    unknown_native = {name: locations for name, locations in native.items()
                      if name not in specs and not (name in native_prefixes
                                                   and any(command.startswith(name) for command in specs))}
    untyped = sorted(name for name, spec in specs.items()
                     if not any(key in spec['returns'] for key in ['type', 'properties', '$ref', 'oneOf', 'anyOf', 'enum', 'const']))
    mapped_commands = {name for entry in mappings.values() for name in entry['commands']}
    features = []
    for row in rows:
        state = states.get(row['id'], {})
        implementation = state.get('implementation', '重新核对' if row['priority'] == '✗' else '待实现')
        entry = mappings.get(row['id'])
        features.append({**row, 'implementation': implementation, 'entrypoint': entry})
    unmapped = [f['id'] for f in features if f['implementation'].startswith(('已实现', '部分')) and f['entrypoint'] is None]
    partial = [identity for identity, entry in mappings.items() if entry['coverage'] != 'complete']
    unfinished = [f['id'] for f in features if not f['implementation'].startswith('已实现')]
    complete = not (unmapped or partial or unfinished or unknown_native or untyped)
    return {
        'scope': 'Development audit; not a claim that all Photoshop editing is implemented or AI-verified',
        'registered_commands': len(specs), 'registered_examples': sum(len(s['examples']) for s in catalog),
        'catalog_sha256': hashlib.sha256(json.dumps(catalog, sort_keys=True, ensure_ascii=False).encode()).hexdigest(),
        'inventory_rows': len(rows), 'mapped_features': len(mappings),
        'unmapped_implemented_features': unmapped, 'partial_entrypoints': partial,
        'unfinished_features': unfinished, 'unknown_native_command_references': unknown_native,
        'native_dispatch_prefixes': native_prefixes,
        'untyped_command_returns': untyped, 'commands_without_feature_links': sorted(set(specs) - mapped_commands),
        'final_audit_complete': complete,
        'limits': ['Native audit finds literal references; dynamic dispatch and private callbacks still need manual review.',
                   'Prompt search/describe and actual invoke/undo are checked in local Rust tests; this report alone is not a live-model quality test.',
                   'Composite inventory rows are not a feature-completion percentage.'],
        'features': features,
    }


def render(report):
    lines = ['# AI 编辑入口覆盖跟踪', '',
             '当前开发期审计快照。完整编辑功能与最终 AI 审计仍未闭环；不得把注册数量当作全部功能覆盖。', '',
             f"真实注册命令 {report['registered_commands']}；参数示例 {report['registered_examples']}；综合功能 {report['inventory_rows']}；已有映射 {report['mapped_features']}。", '',
             f"已实现但未映射 {len(report['unmapped_implemented_features'])}；部分入口 {len(report['partial_entrypoints'])}；原生未知命令引用 {len(report['unknown_native_command_references'])}；未类型化返回 {len(report['untyped_command_returns'])}。", '',
             '配置/密钥及基础设施的宿主管理理由单列；纯 UI 状态不能隐瞒为已具备独立 AI 设置命令。', '',
             '| ID | 功能 | 实现 | AI 入口 | 命令 / 提示示例 / 缺口 |', '|---|---|---|---|---|']
    for feature in report['features']:
        entry = feature['entrypoint']
        if entry:
            kind = {'complete': '已映射', 'partial': '部分入口'}[entry['coverage']]
            if entry['scope'] != 'editing':
                kind += ' · ' + entry['scope']
            detail = '、'.join(f'`{c}`' for c in entry['commands'] + entry.get('meta_tools', []))
            if entry['prompts']:
                detail += ' · ' + entry['prompts'][0]['text']
            if entry['note']:
                detail += ' · ' + entry['note']
        else:
            kind, detail = '待实现/待核对', '保留最终入口与提示核对任务'
        name = feature['name'].replace('|', r'\|')
        lines.append(f"| {feature['id']} | {name} | {feature['implementation']} | {kind} | {detail} |")
    lines += ['', '## 仍需核对', '',
              '- 部分入口：' + '、'.join(report['partial_entrypoints']),
              '- 未类型化返回：' + ('、'.join(f'`{v}`' for v in report['untyped_command_returns']) or '无'),
              '- 未关联功能条目的命令：' + '、'.join(f'`{v}`' for v in report['commands_without_feature_links']),
              '- 动态拼接及纯宿主回调仍需人工逐项复核；已有提示映射不能代替最终模型端到端验收。',
              '- `--require-p0` 检查 P0 实现与入口。`--require-complete` 检查完整清单。', '']
    return '\n'.join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--catalog', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--require-p0', action='store_true')
    parser.add_argument('--require-complete', action='store_true')
    args = parser.parse_args()
    report = audit(json.loads(args.catalog.read_text()))
    outputs = {REPORT: json.dumps(report, ensure_ascii=False, indent=2) + '\n', MARKDOWN: render(report)}
    for path, text in outputs.items():
        if args.check:
            if not path.exists() or path.read_text() != text:
                raise SystemExit(f'Stale AI audit: {path.relative_to(ROOT)}; run scripts/ai-entrypoints.py --catalog PATH')
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
    if report['unknown_native_command_references']:
        raise SystemExit(f"Unknown native command references: {report['unknown_native_command_references']}")
    if args.require_p0:
        gaps = []
        p0_count = 0
        for feature in report['features']:
            if feature['priority'] != 'P0':
                continue
            p0_count += 1
            issues = []
            if feature['implementation'] != '已实现':
                issues.append(feature['implementation'])
            entry = feature['entrypoint']
            if entry is None or entry['coverage'] != 'complete':
                issues.append('AI 入口待核对' if entry is None else 'AI 入口不完整')
            if issues:
                gaps.append(f"{feature['id']} {feature['name']} ({'、'.join(issues)})")
        if report['untyped_command_returns']:
            gaps.append(f"未类型化命令返回: {', '.join(report['untyped_command_returns'])}")
        if gaps:
            raise SystemExit('P0 待核对条目\n' + '\n'.join(gaps))
        print(f'P0: {p0_count} composite entries have complete implementation and entrypoint records.')
    if args.require_complete and not report['final_audit_complete']:
        raise SystemExit('Full editing/AI entrypoint closure is not complete; inspect the audit report.')
    print(f"AI audit: {report['registered_commands']} commands, {report['mapped_features']}/{report['inventory_rows']} composite rows mapped; final closure remains {report['final_audit_complete']}.")


if __name__ == '__main__':
    main()
