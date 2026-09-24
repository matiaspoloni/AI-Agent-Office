//! Scripted scenarios for the demo provider. Pure data: the runner turns
//! these steps into normalized events.

use ao_core::event::ToolCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOp {
    Read,
    Create,
    Modify,
}

#[derive(Debug, Clone)]
pub enum Step {
    Think(&'static str, u64),
    Message(&'static str),
    Tool {
        name: &'static str,
        category: ToolCategory,
        title: &'static str,
        ms: u64,
        ok: bool,
        file: Option<(FileOp, &'static str)>,
    },
    Command {
        command: &'static str,
        ms: u64,
        exit_code: i32,
        output: &'static [&'static str],
    },
    Permission {
        tool: &'static str,
        description: &'static str,
    },
    Usage {
        input: u64,
        output: u64,
        cached: u64,
    },
    Subagents(Vec<SubScript>),
    Compact,
    Error(&'static str),
    Wait(u64),
}

#[derive(Debug, Clone)]
pub struct SubScript {
    pub agent_type: &'static str,
    pub description: &'static str,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub key: &'static str,
    pub prompt: &'static str,
    pub steps: Vec<Step>,
}

/// A preset office member: project folder, title and its rotation of scenarios.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub key: &'static str,
    pub title: &'static str,
    pub cwd: &'static str,
    pub model: &'static str,
    pub scenarios: &'static [&'static str],
}

/// What a running demo session does. Built from a [`Preset`], optionally
/// customized by the user's launch request.
#[derive(Debug, Clone)]
pub struct SessionPlan {
    pub key: String,
    pub title: String,
    pub cwd: String,
    pub model: String,
    pub scenarios: &'static [&'static str],
}

impl From<&Preset> for SessionPlan {
    fn from(p: &Preset) -> Self {
        Self {
            key: p.key.into(),
            title: p.title.into(),
            cwd: p.cwd.into(),
            model: p.model.into(),
            scenarios: p.scenarios,
        }
    }
}

pub const PRESETS: &[Preset] = &[
    Preset {
        key: "nalu",
        title: "Nalu · feature work",
        cwd: "C:\\Projects\\Nalu",
        model: "sim-large",
        scenarios: &["feature", "research"],
    },
    Preset {
        key: "munder",
        title: "Munder Difflin · test fixing",
        cwd: "C:\\Projects\\Munder-Difflin",
        model: "sim-fast",
        scenarios: &["tests", "feature"],
    },
    Preset {
        key: "review",
        title: "Munder Difflin · UI review",
        cwd: "C:\\Projects\\Munder-Difflin\\web",
        model: "sim-large",
        scenarios: &["review", "tests"],
    },
    Preset {
        key: "atlas",
        title: "Atlas · research",
        cwd: "C:\\Projects\\Atlas",
        model: "sim-large",
        scenarios: &["research", "review"],
    },
];

fn tool(name: &'static str, category: ToolCategory, title: &'static str, ms: u64) -> Step {
    Step::Tool {
        name,
        category,
        title,
        ms,
        ok: true,
        file: None,
    }
}

fn file_tool(
    name: &'static str,
    category: ToolCategory,
    title: &'static str,
    ms: u64,
    op: FileOp,
    path: &'static str,
) -> Step {
    Step::Tool {
        name,
        category,
        title,
        ms,
        ok: true,
        file: Some((op, path)),
    }
}

pub fn scenario(key: &str) -> Scenario {
    use ToolCategory::*;
    match key {
        "tests" => Scenario {
            key: "tests",
            prompt: "The CI is red. Make the test suite pass.",
            steps: vec![
                Step::Think("Reading the failing CI log", 1800),
                Step::Command {
                    command: "npm test",
                    ms: 5200,
                    exit_code: 1,
                    output: &[
                        "FAIL src/invoices.test.ts",
                        "  ✕ totals include tax (12 ms)",
                        "Tests: 1 failed, 41 passed",
                    ],
                },
                file_tool(
                    "Read",
                    Read,
                    "Read src/invoices.ts",
                    1200,
                    FileOp::Read,
                    "src/invoices.ts",
                ),
                Step::Think("Tax is applied after rounding — wrong order", 2200),
                file_tool(
                    "Edit",
                    Edit,
                    "Edit src/invoices.ts",
                    2600,
                    FileOp::Modify,
                    "src/invoices.ts",
                ),
                Step::Permission {
                    tool: "Bash",
                    description: "Run: npm install decimal.js --save",
                },
                Step::Command {
                    command: "npm install decimal.js --save",
                    ms: 3000,
                    exit_code: 0,
                    output: &["added 1 package"],
                },
                Step::Command {
                    command: "npm test",
                    ms: 5200,
                    exit_code: 0,
                    output: &["PASS src/invoices.test.ts", "Tests: 42 passed"],
                },
                Step::Usage {
                    input: 48_200,
                    output: 3_900,
                    cached: 31_000,
                },
                Step::Message("All 42 tests pass. Tax is now applied before rounding."),
            ],
        },
        "review" => Scenario {
            key: "review",
            prompt: "Review the dashboard UI and fix accessibility issues.",
            steps: vec![
                tool("Glob", Search, "Find **/*.tsx", 900),
                file_tool(
                    "Read",
                    Read,
                    "Read src/Dashboard.tsx",
                    1400,
                    FileOp::Read,
                    "src/Dashboard.tsx",
                ),
                file_tool(
                    "Read",
                    Read,
                    "Read src/theme.css",
                    1100,
                    FileOp::Read,
                    "src/theme.css",
                ),
                tool("WebFetch", Fetch, "Fetch WCAG contrast guidelines", 2000),
                Step::Think("Buttons lack labels; contrast below 4.5:1", 2000),
                file_tool(
                    "Edit",
                    Edit,
                    "Edit src/Dashboard.tsx",
                    2400,
                    FileOp::Modify,
                    "src/Dashboard.tsx",
                ),
                file_tool(
                    "Edit",
                    Edit,
                    "Edit src/theme.css",
                    1800,
                    FileOp::Modify,
                    "src/theme.css",
                ),
                Step::Command {
                    command: "npm run lint",
                    ms: 3000,
                    exit_code: 0,
                    output: &["✔ No problems"],
                },
                Step::Usage {
                    input: 36_500,
                    output: 2_700,
                    cached: 20_000,
                },
                Step::Message("Added aria-labels and raised contrast on 6 components."),
            ],
        },
        "research" => Scenario {
            key: "research",
            prompt: "Plan the migration from REST to GraphQL.",
            steps: vec![
                Step::Think("Splitting the investigation", 1500),
                Step::Subagents(vec![
                    SubScript {
                        agent_type: "Researcher",
                        description: "Survey current REST endpoints",
                        steps: vec![
                            tool("Grep", Search, "Search router definitions", 1500),
                            file_tool(
                                "Read",
                                Read,
                                "Read api/routes.ts",
                                1800,
                                FileOp::Read,
                                "api/routes.ts",
                            ),
                            Step::Think("34 endpoints, 6 resources", 1500),
                        ],
                    },
                    SubScript {
                        agent_type: "Backend Agent",
                        description: "Prototype the schema",
                        steps: vec![
                            file_tool(
                                "Write",
                                Edit,
                                "Write api/schema.graphql",
                                2600,
                                FileOp::Create,
                                "api/schema.graphql",
                            ),
                            Step::Think("Resolvers map 1:1 to services", 1400),
                        ],
                    },
                    SubScript {
                        agent_type: "QA Agent",
                        description: "Check contract tests coverage",
                        steps: vec![
                            Step::Command {
                                command: "npm test -- api",
                                ms: 4200,
                                exit_code: 0,
                                output: &["Tests: 18 passed"],
                            },
                            Step::Think("Contract tests cover 80% of endpoints", 1200),
                        ],
                    },
                ]),
                Step::Compact,
                file_tool(
                    "Write",
                    Edit,
                    "Write docs/graphql-plan.md",
                    2200,
                    FileOp::Create,
                    "docs/graphql-plan.md",
                ),
                Step::Usage {
                    input: 91_000,
                    output: 7_400,
                    cached: 60_000,
                },
                Step::Message("Migration plan written to docs/graphql-plan.md."),
            ],
        },
        _ => Scenario {
            key: "feature",
            prompt: "Add CSV export to the reports page.",
            steps: vec![
                Step::Think("Understanding the reports module", 1600),
                tool("Grep", Search, "Search \"ReportsPage\"", 1100),
                file_tool(
                    "Read",
                    Read,
                    "Read src/reports/ReportsPage.tsx",
                    1500,
                    FileOp::Read,
                    "src/reports/ReportsPage.tsx",
                ),
                Step::Subagents(vec![SubScript {
                    agent_type: "Explore",
                    description: "Find existing export helpers",
                    steps: vec![
                        tool("Glob", Search, "Find **/export*.ts", 1200),
                        file_tool(
                            "Read",
                            Read,
                            "Read src/lib/export.ts",
                            1600,
                            FileOp::Read,
                            "src/lib/export.ts",
                        ),
                    ],
                }]),
                file_tool(
                    "Write",
                    Edit,
                    "Write src/reports/csv.ts",
                    2600,
                    FileOp::Create,
                    "src/reports/csv.ts",
                ),
                file_tool(
                    "Edit",
                    Edit,
                    "Edit src/reports/ReportsPage.tsx",
                    2200,
                    FileOp::Modify,
                    "src/reports/ReportsPage.tsx",
                ),
                Step::Command {
                    command: "npm test -- reports",
                    ms: 4600,
                    exit_code: 0,
                    output: &["PASS src/reports/csv.test.ts", "Tests: 7 passed"],
                },
                Step::Usage {
                    input: 52_000,
                    output: 4_300,
                    cached: 28_000,
                },
                Step::Message("CSV export added with tests."),
            ],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_scenario_exists() {
        for preset in PRESETS {
            for key in preset.scenarios {
                assert_eq!(scenario(key).key, *key);
            }
        }
    }
}
