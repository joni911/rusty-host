//! TDD-05 — Template rendering.
//!
//! Refs: PRD §3.1 item 6, §4.3. Templates are `<<TOKEN>>` fragments; rendering is
//! substitution only, with no logic. The subtle part is that **values are not
//! re-parsed**, which is what stops a path or hostname containing `<<`/`{{` from
//! being expanded a second time.

use rh_core::template::{render, RenderError};
use std::collections::BTreeMap;

fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn replaces_known_tokens() {
    let out = render("<VirtualHost *:<<PORT>>><<HOSTNAME>>", &vars(&[
        ("PORT", "80"),
        ("HOSTNAME", "demo.test"),
    ]))
    .unwrap();
    assert_eq!(out, "<VirtualHost *:80>demo.test");
}

#[test]
fn leaves_unknown_tokens_verbatim() {
    // A broken template must be visible, not silently wrong.
    let out = render("<<KNOWN>> and <<MYSTERY>>", &vars(&[("KNOWN", "x")])).unwrap();
    assert_eq!(out, "x and <<MYSTERY>>");
}

#[test]
fn token_values_are_not_reparsed() {
    // The injection-style bug: a *value* containing a token must survive as-is.
    let out = render("<<A>>", &vars(&[("A", "<<B>>")])).unwrap();
    assert_eq!(out, "<<B>>", "values must never be re-expanded");
}

#[test]
fn values_containing_braces_and_percent_survive() {
    let out = render("dir=<<DIR>>", &vars(&[("DIR", "C:/x/{{weird}}/100%")])).unwrap();
    assert_eq!(out, "dir=C:/x/{{weird}}/100%");
}

#[test]
fn paths_are_normalised_to_forward_slashes() {
    let out = render("DocumentRoot \"<<DIR>>\"", &vars(&[("DIR", "C:\\laragon\\www")])).unwrap();
    assert_eq!(out, "DocumentRoot \"C:/laragon/www\"");
}

#[test]
fn windows_drive_paths_render_correctly() {
    let out = render("<<DIR>>", &vars(&[("DIR", "C:\\x\\y")])).unwrap();
    assert_eq!(out, "C:/x/y");
    assert!(!out.contains('\\'), "no backslashes may remain");
}

#[test]
fn unix_paths_are_untouched() {
    let out = render("<<DIR>>", &vars(&[("DIR", "/var/www/html")])).unwrap();
    assert_eq!(out, "/var/www/html");
}

#[test]
fn render_is_deterministic() {
    let v = vars(&[("A", "1"), ("B", "2")]);
    let first = render("<<A>>-<<B>>", &v).unwrap();
    for _ in 0..10 {
        assert_eq!(render("<<A>>-<<B>>", &v).unwrap(), first);
    }
}

#[test]
fn empty_template_renders_empty() {
    assert_eq!(render("", &vars(&[])).unwrap(), "");
}

#[test]
fn template_without_tokens_is_returned_unchanged() {
    let tpl = "# plain config\nkey = value\n";
    assert_eq!(render(tpl, &vars(&[("UNUSED", "x")])).unwrap(), tpl);
}

#[test]
fn multiline_templates_preserve_line_structure() {
    let tpl = "[section]\na = <<A>>\nb = <<B>>\n";
    let out = render(tpl, &vars(&[("A", "1"), ("B", "2")])).unwrap();
    assert_eq!(out.lines().count(), tpl.lines().count());
}

#[test]
fn crlf_templates_keep_crlf() {
    let tpl = "a = <<A>>\r\nb = 2\r\n";
    let out = render(tpl, &vars(&[("A", "1")])).unwrap();
    assert_eq!(out, "a = 1\r\nb = 2\r\n");
}

#[test]
fn rejects_malformed_token() {
    // An unterminated `<<` is a template bug worth reporting.
    let err = render("a = <<UNCLOSED", &vars(&[])).unwrap_err();
    assert!(matches!(err, RenderError::MalformedToken { .. }), "got {err:?}");
}

#[test]
fn hash_token_is_still_unknown() {
    // `<<` opening but a lone `>` inside must not be treated as a token.
    let out = render("a > b", &vars(&[])).unwrap();
    assert_eq!(out, "a > b");
}

// --------------------------------------------------------------- snapshots

#[test]
fn renders_apache_vhost_golden() {
    let tpl = include_str!("../../../templates/vhost.apache.tpl");
    let out = render(
        tpl,
        &vars(&[
            ("PORT", "80"),
            ("HOSTNAME", "demo.test"),
            ("PROJECT_DIR", "C:\\rh\\www\\demo"),
        ]),
    )
    .unwrap();
    assert!(out.contains("<VirtualHost *:80>"));
    assert!(out.contains("ServerName demo.test"));
    assert!(out.contains("ServerAlias www.demo.test"));
    assert!(out.contains("DocumentRoot \"C:/rh/www/demo\""));
    assert!(out.contains("AllowOverride All"), "must permit .htaccess");
    assert!(out.contains("</VirtualHost>"));
}

#[test]
fn renders_nginx_server_golden() {
    let tpl = include_str!("../../../templates/vhost.nginx.tpl");
    let out = render(
        tpl,
        &vars(&[
            ("PORT", "80"),
            ("HOSTNAME", "demo.test"),
            ("PROJECT_DIR", "C:\\rh\\www\\demo"),
            ("PHP_PORT", "9000"),
        ]),
    )
    .unwrap();
    assert!(out.contains("listen 80;"));
    assert!(out.contains("server_name demo.test www.demo.test;"));
    assert!(out.contains("root \"C:/rh/www/demo\";"));
    assert!(out.contains("fastcgi_pass 127.0.0.1:9000;"));
    assert!(out.contains("index index.php"));
}

#[test]
fn renders_php_ini_golden() {
    let tpl = include_str!("../../../templates/php.ini.tpl");
    let out = render(
        tpl,
        &vars(&[
            ("EXT_DIR", "C:\\rh\\bin\\php\\8.4.27\\ext"),
            ("TMP_DIR", "C:\\rh\\tmp"),
            ("ERROR_LOG", "C:\\rh\\data\\logs\\php-error.log"),
        ]),
    )
    .unwrap();
    assert!(out.contains("extension_dir = \"C:/rh/bin/php/8.4.27/ext\""));
    assert!(out.contains("sys_temp_dir = \"C:/rh/tmp\""));
    assert!(out.contains("error_log = \"C:/rh/data/logs/php-error.log\""));
    assert!(out.contains("upload_tmp_dir"));
    assert!(out.contains("date.timezone"));
}

#[test]
fn renders_my_ini_golden() {
    let tpl = include_str!("../../../templates/my.ini.tpl");
    let out = render(
        tpl,
        &vars(&[
            ("BASEDIR", "C:\\rh\\bin\\mariadb\\11.8.4"),
            ("DATADIR", "C:\\rh\\data\\mariadb"),
            ("PORT", "3306"),
        ]),
    )
    .unwrap();
    assert!(out.contains("basedir=C:/rh/bin/mariadb/11.8.4"));
    assert!(out.contains("datadir=C:/rh/data/mariadb"));
    assert!(out.contains("port=3306"));
    assert!(out.contains("[mysqld]"));
}

#[test]
fn renders_httpd_conf_golden() {
    let tpl = include_str!("../../../templates/httpd.conf.tpl");
    let out = render(
        tpl,
        &vars(&[
            ("SERVER_ROOT", "C:\\rh\\bin\\apache\\2.4.69"),
            ("WWW_DIR", "C:\\rh\\www"),
            ("PORT", "80"),
            ("SITES_DIR", "C:\\rh\\conf\\apache\\sites-enabled"),
        ]),
    )
    .unwrap();
    assert!(out.contains("ServerRoot \"C:/rh/bin/apache/2.4.69\""));
    assert!(out.contains("DocumentRoot \"C:/rh/www\""));
    assert!(out.contains("Listen 80"));
    assert!(
        out.contains("IncludeOptional \"C:/rh/conf/apache/sites-enabled/*.conf\""),
        "generated vhosts must be included"
    );
}
