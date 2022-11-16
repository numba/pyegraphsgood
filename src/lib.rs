use egg::{AstDepth, AstSize, CostFunction, Extractor, Pattern, Rewrite, Runner, SymbolLang};
use pyo3::{exceptions::PyValueError, prelude::*};

#[pyclass]
struct Language(Vec<Rewrite<SymbolLang, ()>>);
#[pyclass]
#[derive(Clone)]
struct RewriteRule(Vec<Rewrite<SymbolLang, ()>>);

impl Language {
    fn simplify_with_cost<C: CostFunction<SymbolLang>>(
        &self,
        input: &str,
        cost_function: C,
        py: Python,
    ) -> PyResult<PyObject>
    where
        C::Cost: ToPyObject,
    {
        match input.parse() {
            Ok(expr) => {
                let runner = Runner::default().with_expr(&expr).run(&self.0);
                let extractor = Extractor::new(&runner.egraph, cost_function);
                let (best_cost, best) = extractor.find_best(runner.roots[0]);
                Ok((best_cost, best.to_string()).to_object(py))
            }
            Err(e) => Err(PyErr::from_value(
                PyValueError::new_err(e.to_string()).value(py),
            )),
        }
    }
}

#[pymethods]
impl Language {
    #[new]
    fn new(rules: Vec<PyRef<RewriteRule>>) -> Self {
        Language(
            rules
                .iter()
                .flat_map(|rule| rule.0.iter().cloned())
                .collect(),
        )
    }
    fn simplify(&self, input: &str, cost: &str) -> PyResult<PyObject> {
        Python::with_gil(|py| match cost {
            "ast-size" => self.simplify_with_cost(input, AstSize, py),
            "ast-depth" => self.simplify_with_cost(input, AstDepth, py),
            _ => Err(PyErr::from_value(
                PyValueError::new_err(format!("Unknown cost function {}", cost)).value(py),
            )),
        })
    }
}
#[pymethods]
impl RewriteRule {
    #[new]
    fn new(name: String, searcher: &str, applier: &str, symmetric: bool) -> PyResult<Self> {
        Python::with_gil(|py| {
            let searcher = searcher
                .parse::<Pattern<SymbolLang>>()
                .map_err(|e| PyErr::from_value(PyValueError::new_err(e.to_string()).value(py)))?;
            let applier = applier
                .parse::<Pattern<SymbolLang>>()
                .map_err(|e| PyErr::from_value(PyValueError::new_err(e.to_string()).value(py)))?;
            let mut rules = Vec::with_capacity(if symmetric { 2 } else { 1 });
            if symmetric {
                rules.push(
                    Rewrite::new(format!("{}-rev", name), applier.clone(), searcher.clone())
                        .map_err(|e| {
                            PyErr::from_value(PyValueError::new_err(e.to_string()).value(py))
                        })?,
                );
            }
            rules.push(
                Rewrite::new(name, searcher, applier).map_err(|e| {
                    PyErr::from_value(PyValueError::new_err(e.to_string()).value(py))
                })?,
            );
            Ok(RewriteRule(rules))
        })
    }
}

#[pymodule]
fn pyegraphsgood(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Language>()?;
    m.add_class::<RewriteRule>()?;
    Ok(())
}
