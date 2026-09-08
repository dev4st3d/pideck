"""Synthetic unit tests for the comparison utility, not native UI verification."""
import importlib.util
import unittest
from pathlib import Path
from PIL import Image

spec = importlib.util.spec_from_file_location("compare_native", Path(__file__).with_name("compare-native.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class CompareTests(unittest.TestCase):
    def test_equal_pixels_are_exact(self):
        image = Image.new("RGB", (8, 8), (15, 22, 30))
        metrics, diff = module.compare(image, image.copy())
        self.assertTrue(metrics["pixel_exact"])
        self.assertEqual(metrics["different_pixels"], 0)
        self.assertIsNone(diff.getbbox())

    def test_one_changed_channel_is_counted_without_tolerance(self):
        expected = Image.new("RGB", (2, 2))
        actual = expected.copy()
        actual.putpixel((1, 1), (3, 0, 0))
        metrics, _ = module.compare(expected, actual)
        self.assertFalse(metrics["pixel_exact"])
        self.assertEqual(metrics["different_pixels"], 1)
        self.assertEqual(metrics["exact_pixel_fraction"], 0.75)
        self.assertEqual(metrics["mean_absolute_rgb_error_0_255"], 0.25)
        self.assertEqual(metrics["max_channel_error_0_255"], 3)
        self.assertEqual(metrics["difference_bounds"], (1, 1, 2, 2))

    def test_size_mismatch_is_not_rescaled(self):
        with self.assertRaisesRegex(ValueError, "no scaling permitted"):
            module.compare(Image.new("RGB", (8, 8)), Image.new("RGB", (16, 16)))

    def test_all_six_reference_names_are_required(self):
        self.assertEqual(len(module.NAMES), 6)
        self.assertEqual(len(set(module.NAMES)), 6)
        self.assertIn("06-midnight-dark-ongoing-expanded.png", module.NAMES)


if __name__ == "__main__":
    unittest.main()
