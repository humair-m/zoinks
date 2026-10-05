"""Python-side tests for the args parser — mirrors src/args.rs."""

import pytest

from zoinks.args import parse_args, HELP


def test_help_short_and_long():
    assert parse_args(["-h"]).help is True
    assert parse_args(["--help"]).help is True


def test_version_short_and_long():
    assert parse_args(["-v"]).version is True
    assert parse_args(["--version"]).version is True


def test_positional_url():
    parsed = parse_args(["https://youtu.be/x"])
    assert parsed.initial_url == "https://youtu.be/x"
    assert parsed.error is None


def test_too_many_urls_errors():
    parsed = parse_args(["https://a", "https://b"])
    assert parsed.error == "expected a single url"


def test_theme_value_and_eq_form():
    assert parse_args(["--theme", "dark"]).theme_mode == "dark"
    assert parse_args(["--theme=light"]).theme_mode == "light"


def test_unknown_theme_errors():
    assert parse_args(["--theme", "neon"]).error is not None


def test_unknown_flag_errors():
    parsed = parse_args(["--nope"])
    assert parsed.error is not None
    assert "unknown option" in parsed.error


def test_help_text_shape():
    assert "zoinks" in HELP
    assert "Usage" in HELP
    assert "Examples" in HELP


def test_no_args_returns_default():
    parsed = parse_args([])
    assert parsed.help is False
    assert parsed.version is False
    assert parsed.initial_url is None
    assert parsed.theme_mode is None
    assert parsed.error is None


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
