use egg::{
    Applier, AstDepth, AstSize, CostFunction, Extractor, FromOp, Id, Pattern, RecExpr, Rewrite,
    Runner, Searcher, SymbolLang,
};
use pyo3::types::IntoPyDict;
use pyo3::{exceptions::PyValueError, prelude::*};

use std::sync::Arc;

#[pyclass]
struct Language(Vec<Rewrite<SymbolLang, ()>>);
struct Input(RecExpr<SymbolLang>);
#[pyclass]
#[derive(Clone)]
struct RewriteRule(Vec<Rewrite<SymbolLang, ()>>);

#[derive(Clone)]
struct PyConditionalApplier {
    condition: PyObject,
    searcher: Arc<dyn Searcher<SymbolLang, ()> + Sync + Send>,
    applier: Arc<dyn Applier<SymbolLang, ()> + Sync + Send>,
}
struct ProxySearcher(Arc<dyn Searcher<SymbolLang, ()> + Sync + Send>);

impl Language {
    fn simplify_with_cost<C: CostFunction<SymbolLang>>(
        &self,
        input: Input,
        cost_function: C,
        py: Python,
    ) -> PyResult<PyObject>
    where
        C::Cost: ToPyObject,
    {
        let runner = Runner::default().with_expr(&input.0).run(&self.0);
        let extractor = Extractor::new(&runner.egraph, cost_function);
        let (best_cost, best) = extractor.find_best(runner.roots[0]);

        let result = best.as_ref().last().unwrap();
        Ok((best_cost, {
            use egg::Language;
            (
                result.op.as_str(),
                result
                    .children()
                    .iter()
                    .map(|child| convert(*child, &runner.egraph, py))
                    .collect::<Vec<_>>(),
            )
                .to_object(py)
        })
            .to_object(py))
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
    fn simplify(&self, input: Input, cost: &str) -> PyResult<PyObject> {
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
    fn only_when(&self, condition: Py<PyAny>) -> PyResult<Self> {
        Python::with_gil(|py| {
            Ok(RewriteRule(
                self.0
                    .iter()
                    .map(|rule| {
                        Rewrite::new(
                            format!("{}-cond", &rule.name),
                            ProxySearcher(rule.searcher.clone()),
                            PyConditionalApplier {
                                applier: rule.applier.clone(),
                                condition: condition.clone(),
                                searcher: rule.searcher.clone(),
                            },
                        )
                        .map_err(|e| {
                            PyErr::from_value(PyValueError::new_err(e.to_string()).value(py))
                        })
                    })
                    .collect::<PyResult<Vec<_>>>()?,
            ))
        })
    }
}

impl Applier<SymbolLang, ()> for PyConditionalApplier {
    fn apply_one(
        &self,
        egraph: &mut egg::EGraph<SymbolLang, ()>,
        eclass: egg::Id,
        subst: &egg::Subst,
        searcher_ast: Option<&egg::PatternAst<SymbolLang>>,
        rule_name: egg::Symbol,
    ) -> Vec<egg::Id> {
        let result = Python::with_gil(|py| {
            let args = self
                .searcher
                .vars()
                .into_iter()
                .flat_map(|var| {
                    subst
                        .get(var)
                        .map(|value| {
                            let mut name = var.to_string();
                            // Strip leading question mark
                            name.remove(0);

                            (name, convert(*value, egraph, py))
                        })
                        .into_iter()
                })
                .into_py_dict(py);
            match self
                .condition
                .call(py, (), Some(args))
                .and_then(|r| r.is_true(py))
            {
                Ok(result) => result,
                Err(e) => {
                    eprint!("Python error happen in egraph condition: {}", e);
                    false
                }
            }
        });
        if result {
            self.applier
                .apply_one(egraph, eclass, subst, searcher_ast, rule_name)
        } else {
            vec![]
        }
    }
}
impl Searcher<SymbolLang, ()> for ProxySearcher {
    fn search_eclass_with_limit(
        &self,
        egraph: &egg::EGraph<SymbolLang, ()>,
        eclass: egg::Id,
        limit: usize,
    ) -> Option<egg::SearchMatches<SymbolLang>> {
        self.0.search_eclass_with_limit(egraph, eclass, limit)
    }

    fn vars(&self) -> Vec<egg::Var> {
        self.0.vars()
    }
}

impl<'source> FromPyObject<'source> for Input {
    fn extract(ob: &'source PyAny) -> PyResult<Self> {
        Python::with_gil(|py| match ob.extract::<(&str, Vec<PyObject>)>() {
            Ok((name, children)) => {
                fn unpack(
                    expr: &mut RecExpr<SymbolLang>,
                    value: PyObject,
                    py: Python,
                ) -> PyResult<Id> {
                    let (name, children) = value.extract::<(&str, Vec<PyObject>)>(py)?;
                    let children = children
                        .into_iter()
                        .map(|child| unpack(expr, child, py))
                        .collect::<PyResult<Vec<_>>>()?;
                    Ok(expr.add(SymbolLang::from_op(name, children).map_err(|e| {
                        PyErr::from_value(PyValueError::new_err(e.to_string()).value(py))
                    })?))
                }
                let mut expr = RecExpr::default();
                let children = children
                    .into_iter()
                    .map(|child| unpack(&mut expr, child, py))
                    .collect::<PyResult<Vec<_>>>()?;
                expr.add(SymbolLang::from_op(name, children).map_err(|e| {
                    PyErr::from_value(PyValueError::new_err(e.to_string()).value(py))
                })?);
                Ok(Input(expr))
            }
            Err(_) => match ob.extract::<&str>()?.parse() {
                Ok(expr) => Ok(Input(expr)),
                Err(e) => Err(PyErr::from_value(
                    PyValueError::new_err(e.to_string()).value(py),
                )),
            },
        })
    }
}
fn convert(id: Id, egraph: &egg::EGraph<SymbolLang, ()>, py: Python) -> PyObject {
    egraph[id]
        .nodes
        .iter()
        .map(|v| {
            use egg::Language;
            (
                v.op.as_str(),
                v.children()
                    .iter()
                    .map(|child| convert(*child, egraph, py))
                    .collect::<Vec<_>>(),
            )
                .to_object(py)
        })
        .next()
        .to_object(py)
}

#[pymodule]
fn pyegraphsgood(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Language>()?;
    m.add_class::<RewriteRule>()?;
    Ok(())
}
