import unittest
import json
from performance_report import compare, summarize, CONFIG
from run_suite import parse_performance


def run(seconds, ok=True):
    report = {k: "same" for k in CONFIG}
    report["no_verify"] = False
    report["results"] = [{"id": "T1", "pass": ok, "rc": 0 if ok else 1,
                          "timed_out": False, "completion_s": seconds}]
    return report


class ComparisonTests(unittest.TestCase):
    def test_unfinished_scopes_remain_visible_without_becoming_zero_cost(self):
        report = run(180, False)
        events = [
            'stage="inference_headers" timing_id=9 outcome="started"',
            'stage="inference_headers" timing_id=9 outcome="succeeded"',
            'stage="inference_stream" timing_id=10 outcome="started"',
        ]
        report["results"][0]["performance_stages"] = parse_performance("\n".join(
            json.dumps({"component_name":"lokai_performance", "outcome":event,
                        "metrics":{"duration_ms":0}}) for event in events))
        summary = summarize(report)
        self.assertEqual(summary["unclosed_observed_stages"], {"inference_stream":1})
        self.assertNotIn("inference_stream", summary["observed_stages"])

    def test_stage_totals_keep_failed_attempts_and_do_not_sum_children(self):
        report = run(40, False)
        report["results"][0]["performance_stages"] = [
            {"stage": "inference_backend_total", "outcome": "succeeded", "duration_ms": 30},
            {"stage": "inference_decode", "outcome": "succeeded", "duration_ms": 20},
            {"stage": "inference", "outcome": "interrupted", "duration_ms": 40},
        ]
        stages = summarize(report)["observed_stages"]
        self.assertEqual(stages["inference_backend_total"]["total_ms"], 30)
        self.assertEqual(stages["inference_decode"]["samples"], 1)
        self.assertNotIn("inference", stages)

    def test_backend_phases_are_retained_without_payloads(self):
        for stage in ("inference_load", "inference_prefill", "inference_decode", "inference_backend_total"):
            event = {"component_name": "lokai_performance", "outcome": f'stage="{stage}" outcome="succeeded"', "metrics": {"duration_ms": 12.5}}
            self.assertEqual(parse_performance(json.dumps(event))[0]["stage"], stage)

    def test_malformed_metrics_are_ignored(self):
        for value in (True, float("nan"), float("inf"), -1, "12"):
            event = {"component_name": "lokai_performance", "outcome": 'stage="tool" outcome="succeeded"', "metrics": {"duration_ms": value}}
            self.assertEqual(parse_performance(json.dumps(event)), [])
    def test_only_numeric_stage_data_is_retained(self):
        event = {"component_name": "lokai_performance", "outcome": 'stage="tool" outcome="succeeded" secret=discard-me', "metrics": {"duration_ms": 1.25}}
        parsed = parse_performance("not json\n" + json.dumps(event))
        self.assertEqual(parsed, [{"stage": "tool", "outcome": "succeeded", "duration_ms": 1.25}])
        self.assertNotIn("discard-me", json.dumps(parsed))

    def test_successful_speedup(self):
        self.assertEqual(compare(run(40), run(10))["effort_speedup"], 4)

    def test_fast_failure_is_not_an_improvement(self):
        report = compare(run(40), run(1, False))
        self.assertTrue(report["observed_quality_regression"])
        self.assertIsNone(report["effort_speedup"])

    def test_no_success_has_no_speedup(self):
        self.assertIsNone(compare(run(40, False), run(10, False))["effort_speedup"])

    def test_context_change_is_rejected(self):
        after = run(10)
        after["num_ctx"] = "smaller"
        with self.assertRaises(ValueError):
            compare(run(40), after)

    def test_failure_effort_is_counted(self):
        before, after = run(40), run(10)
        before["results"].append(run(40, False)["results"][0])
        after["results"].append(run(30, False)["results"][0])
        self.assertEqual(compare(before, after)["effort_speedup"], 2)


if __name__ == "__main__":
    unittest.main()
