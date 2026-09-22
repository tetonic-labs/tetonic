use super::*;

#[test]
fn powershell_treats_adversarial_paths_as_literal_data() {
    // Real PowerShell binding, fake firewall cmdlets: no elevation or rule changes.
    let stub = r#"
function New-NetFirewallRule {
    param($ErrorAction,$DisplayName,$Name,$Direction,$Action,$Program,$Profile)
    if ($Program -cne $env:LOKAI_FW_PROGRAM) { exit 91 }
    if ($Name -cne $env:LOKAI_FW_RULE) { exit 92 }
    if ($DisplayName -cne $Name) { exit 93 }
    if ($Direction -ne 'Outbound' -or $Action -ne 'Block' -or $Profile -ne 'Any') { exit 94 }
    $script:called = $true
}
"#;
    for path in [
        r"C:\O'Brien\tool.exe",
        r"C:\'; exit 99; #\tool.exe",
        r"C:\$(exit 99);`quoted`&[abc]\tool.exe",
        "C:\\工具\\tool.exe",
    ] {
        let script = format!("{stub}\n{INSTALL_SCRIPT}\nif (-not $script:called) {{ exit 95 }}");
        run_command(
            firewall_command(&script, "LokaiSandboxDeny-test", Some(Path::new(path))).unwrap(),
        )
        .unwrap();
    }
}

#[test]
fn removal_uses_exact_owned_rule_as_data() {
    let script = format!(
        r#"
function Get-NetFirewallRule {{
    param($Name,$ErrorAction)
    if ($Name -cne $env:LOKAI_FW_RULE) {{ exit 91 }}
    [pscustomobject]@{{Name=$Name}}
}}
function Remove-NetFirewallRule {{
    param([Parameter(ValueFromPipeline=$true)]$InputObject)
    process {{ if ($InputObject.Name -cne $env:LOKAI_FW_RULE) {{ exit 92 }}; $script:called=$true }}
}}
{REMOVE_SCRIPT}
if (-not $script:called) {{ exit 93 }}
"#
    );
    run_command(firewall_command(&script, "LokaiSandboxDeny-test", None).unwrap()).unwrap();
}

#[test]
fn system_powershell_and_constant_script_are_used() {
    let cmd = firewall_command(
        INSTALL_SCRIPT,
        "safe-name",
        Some(Path::new("C:\\O'Brien\\tool.exe")),
    )
    .unwrap();
    assert!(Path::new(cmd.get_program()).is_absolute());
    assert_eq!(cmd.get_args().last().unwrap(), INSTALL_SCRIPT);
    assert!(cmd.get_envs().any(|(key, value)| key == "LOKAI_FW_PROGRAM"
        && value == Some(std::ffi::OsStr::new("C:\\O'Brien\\tool.exe"))));
}
