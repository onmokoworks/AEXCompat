import unittest
from geometry_matrix import cases, expected_result


class AdmissionTests(unittest.TestCase):
    def test_negatives_require_the_declared_admission_boundary(self):
        for case in cases():
            if case.expected != 'reject':
                continue
            good = dict(exit_code=2, frame=None, setup_errors=None,
                        stderr='aex_guest_error: '+case.rejection+'\n')
            self.assertTrue(expected_result(case, good))
            for field, value in [('exit_code',1), ('exit_code','timeout'),
                                 ('exit_code',-9), ('stderr','aex_guest_error: read AEX: missing'),
                                 ('frame',{'status':'error'}),('setup_errors',{'global_setup_error':0})]:
                self.assertFalse(expected_result(case, dict(good, **{field:value})))

    def test_success_requires_clean_exit_and_live_invariants(self):
        case=next(cases())
        good=dict(exit_code=0,status='ok',invariants_ok=True)
        self.assertTrue(expected_result(case,good))
        for field,value in [('exit_code',2),('status','reject'),('invariants_ok',False)]:
            self.assertFalse(expected_result(case,dict(good,**{field:value})))

    def test_typed_input_is_sampled_and_explicit_checkout_is_rebased(self):
        by_name={c.name:c for c in cases()}
        self.assertEqual(by_name['downsample-ceil-8'].world(),(21,11))
        self.assertEqual(by_name['checkout-explicit-8'].world(),(11,7))
        self.assertEqual(by_name['checkout-auto-8'].world(),(17,11))
        self.assertEqual(by_name['4k-32'].world(),(3840,2160))


if __name__=='__main__':
    unittest.main()
