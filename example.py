from pyegraphsgood import Language, RewriteRule

print(Language([RewriteRule("mul-commutes",
                            "(* ?x ?y)",
                            "(* ?y ?x)",
                            False),
                RewriteRule("mul-two",
                            "(* ?x 2)",
                            "(<< ?x 1)",
                            False)]).simplify("(/ (* 2 a) 2)", "ast-size"))
