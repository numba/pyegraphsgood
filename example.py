from pyegraphsgood import Language, RewriteRule


def cond_func(**kwargs):
    print(kwargs)
    return True


lang = Language(
    [
        RewriteRule(
            "mul-commutes",
            "(* ?x ?y)",
            "(* ?y ?x)",
            False),
        RewriteRule(
            "mul-two",
            "(* ?x 2)",
            "(<< ?x 1)",
            False).only_when(cond_func)])

print(
    lang.simplify(
        "(/ (* 2 a) 2)",
        "ast-size"))
print(
    lang.simplify(
        ("/", [("*", [("2", []), ("a", [])]), ("2", [])]),
        "ast-size"))
