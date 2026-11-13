"""Source-only checks for the proposal. Parent executes; not GPU validation.

From this directory: python3 -m unittest test_engraving_factor.py
"""
from pathlib import Path
import unittest
import engraving_factor as proposal


SOURCE_PATH = Path(__file__).resolve().parents[2] / "Procedures/Shaders/workstation.wgsl"


class EngravingFactorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.source = SOURCE_PATH.read_text()

    def test_default_is_identity(self):
        self.assertIs(proposal.transform(self.source), self.source)
        self.assertEqual(proposal.transform(self.source, "off"), self.source)

    def test_metadata_scope_and_original_sampling(self):
        result = proposal.transform(self.source, "metadata")
        for name in ("gemDensityTexel", "gemDensityLevel", "gemEngravedDensity",
                     "gemControlDensityTexel", "gemControlDensityLevel",
                     "gemControlDensity", "gemControlEngravingLight",
                     "gemTimelineEngravingLight"):
            self.assertEqual(proposal.function(result, name),
                             proposal.function(self.source, name))
        self.assertIn("fract(lod)", proposal.GLYPH_PREPARE)
        self.assertIn("fract(lod)", proposal.CONTROL_PREPARE)
        self.assertIn("point-prepared.region.xy-shift*prepared.fractureScale",
                      proposal.function(result, "gemFactorControlDensity"))
        for name in ("gemTimelineEngraving", "gemPluginNavigationEngraving"):
            self.assertEqual(proposal.function(result, name).count(
                "let glyphFilter=gemFactorMakeGlyphFilter("), 1)

    def test_cosine_preserves_contributing_body(self):
        result = proposal.transform(self.source, "cosine")
        for name in ("gemControlEngravingLight", "gemTimelineEngravingLight"):
            before = proposal.function(self.source, name)
            after = proposal.function(result, name)
            self.assertIn("        if(cosine>0.0){\n        var exitHit:GemHit=", after)
            recovered = after.replace("        if(cosine>0.0){\n", "", 1)
            recovered = recovered.replace("        }\n        }\n    }\n    return illumination",
                                          "        }\n    }\n    return illumination", 1)
            self.assertEqual(recovered, before)

    def test_zero_is_exact_alpha_and_preserves_transmission(self):
        result = proposal.transform(self.source, "navigation-zero")
        function = proposal.function(result, "gemPluginNavigationEngraving")
        self.assertIn("if(alpha==0.0&&gemFactorZeroLightSafe(", function)
        self.assertIn("result+=transmission*alpha*vec3<f32>(0.0);", function)
        self.assertIn("transmission*=1.0-alpha;", function)
        self.assertEqual(proposal.function(result, "gemControlEngravingLight"),
                         proposal.function(self.source, "gemControlEngravingLight"))

    def test_variants_combine_and_reject_repeat(self):
        result = proposal.transform(self.source, "all")
        for name in ("gemFactorGlyphDensity", "gemFactorControlDensity",
                     "gemFactorZeroLightSafe"):
            self.assertIn("fn " + name + "(", result)
        with self.assertRaises(ValueError):
            proposal.transform(result, "all")

    def test_changed_markers_fail_closed(self):
        with self.assertRaises(ValueError):
            proposal.transform(self.source, "unknown")
        with self.assertRaises(ValueError):
            proposal.transform(self.source.replace("fn gemControlDensity(",
                                                   "fn gemControlDensityChanged("), "metadata")
        with self.assertRaises(ValueError):
            proposal.transform(self.source.replace("if(t>0.0000001&&t<hit.distance){",
                                                   "if(t>0.0&&t<hit.distance){"), "navigation-zero")


if __name__ == "__main__":
    unittest.main()
