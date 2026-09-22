import os

path = r'crates\lokai-tools\src\process_executor.rs'
with open(path, 'r', encoding='utf-8') as f:
    lines = f.read()

lines = lines.replace('Some("echo".into())', 'Some("cmd.exe".into())')
lines = lines.replace('vec!["arg1 arg2".into()]', 'vec!["/c".into(), "echo".into(), "arg1 arg2".into()]')
lines = lines.replace('vec!["hello; rm -rf /".into()]', 'vec!["/c".into(), "echo".into(), "hello; rm -rf /".into()]')
lines = lines.replace('Some("pwd".into())', 'Some("cmd.exe".into())')
lines = lines.replace('arguments: vec![],\n                shell_identity', 'arguments: vec!["/c".into(), "cd".into()],\n                shell_identity')

with open(path, 'w', encoding='utf-8') as f:
    f.write(lines)
