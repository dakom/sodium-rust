use topological_sort::TopologicalSort;
use std::any::Any;
use std::collections::HashMap;
use std::collections::HashSet;
use std::marker::PhantomData;

pub struct FrpContext<ENV> {
    free_cell_id: u32,
    cell_map: HashMap<u32,CellImpl<ENV,Box<Any>>>,
    cells_to_be_updated: HashSet<u32>,
    change_notifiers: Vec<Box<Fn(&mut ENV)>>,
    transaction_depth: u32
}

pub trait WithFrpContext<ENV> {
    fn with_frp_context<F>(&self, &mut ENV, k: F)
    where F: FnOnce(&mut FrpContext<ENV>);
}

impl<ENV: 'static> FrpContext<ENV> {

    pub fn new() -> FrpContext<ENV> {
        FrpContext {
            free_cell_id: 0,
            cell_map: HashMap::new(),
            cells_to_be_updated: HashSet::new(),
            change_notifiers: Vec::new(),
            transaction_depth: 0
        }
    }

    pub fn transaction<F,F2>(env: &mut ENV, with_frp_context: &F, k: F2)
    where
    F:WithFrpContext<ENV>, F2: FnOnce(&mut ENV, &F),
    {
        with_frp_context.with_frp_context(
            env,
            |frp_context| {
                frp_context.transaction_depth = frp_context.transaction_depth + 1;
            }
        );
        k(env, with_frp_context);
        let mut final_transaction_depth = 0;
        with_frp_context.with_frp_context(
            env,
            |frp_context| {
                frp_context.transaction_depth = frp_context.transaction_depth - 1;
                final_transaction_depth = frp_context.transaction_depth;
            }
        );
        if final_transaction_depth == 0 {
            FrpContext::propergate(env, with_frp_context);
        }
    }

    fn propergate<F>(env: &mut ENV, with_frp_context: &F)
    where F:WithFrpContext<ENV>
    {
        let mut ts = TopologicalSort::<u32>::new();
        let mut change_notifiers: Vec<Box<Fn(&mut ENV)>> = Vec::new();
        let change_notifiers2: *mut Vec<Box<Fn(&mut ENV)>> = &mut change_notifiers;
        with_frp_context.with_frp_context(
            env,
            move |frp_context| {
                frp_context.transaction_depth = frp_context.transaction_depth + 1;
                for cell_to_be_updated in &frp_context.cells_to_be_updated {
                    if let &Some(cell) = &frp_context.cell_map.get(cell_to_be_updated) {
                        for dependent_cell in &cell.dependent_cells {
                            if frp_context.cells_to_be_updated.contains(dependent_cell) {
                                ts.add_dependency(cell.id, dependent_cell.clone());
                            }
                        }
                    }
                }
                loop {
                    let next_op = ts.pop();
                    match next_op {
                        Some(cell_id) => {
                            frp_context.update_cell(&cell_id);
                        },
                        None => break
                    }
                }
                frp_context.transaction_depth = frp_context.transaction_depth - 1;
                unsafe { (*change_notifiers2).append(&mut frp_context.change_notifiers) };
            }
        );
        for change_notifier in change_notifiers {
            change_notifier(env);
        }
    }

    fn update_cell(&mut self, cell_id: &u32)
    {
        let value;
        if let Some(cell) = self.cell_map.get(cell_id) {
            let update_fn = &cell.update_fn;
            value = update_fn(self);
        } else {
            return;
        }
        let mut notifiers_to_add: Vec<Box<Fn(&mut ENV)>> = Vec::new();
        if let Some(cell) = self.cell_map.get_mut(cell_id) {
            cell.value = value;
            let cell2: *const CellImpl<ENV,Box<Any>> = cell;
            notifiers_to_add.push(Box::new(
                move |env| {
                    unsafe {
                        let ref cell3: CellImpl<ENV,Box<Any>> = *cell2;
                        for observer in cell3.observer_map.values() {
                            observer(env, &cell3.value);
                        }
                    }
                }
            ));
        }
        self.change_notifiers.append(&mut notifiers_to_add);
    }

    fn mark_all_decendent_cells_for_update(&mut self, cell_id: u32, visited: &mut HashSet<u32>) {
        visited.insert(cell_id);
        let mut dependent_cells: Vec<u32> = Vec::new();
        match self.cell_map.get(&cell_id) {
            Some(cell) => {
                loop {
                    for dependent_cell in &cell.dependent_cells {
                        dependent_cells.push(dependent_cell.clone());
                    }
                }
            },
            None => ()
        }
        loop {
            let dependent_cell_op = dependent_cells.pop();
            match dependent_cell_op {
                Some(dependent_cell) => {
                    if visited.contains(&dependent_cell) {
                        self.cells_to_be_updated.insert(dependent_cell);
                        self.mark_all_decendent_cells_for_update(dependent_cell, visited);
                    }
                },
                None => break
            }
        }
    }
}

pub trait Cell<ENV,A> {

    fn current_value<'a>(&'a self) -> &'a A;

    fn observe<F,F2>(&self, env: &mut ENV, with_frp_context: &F, observer: F2) -> Box<Fn(&mut Self)>
    where
    F:WithFrpContext<ENV>,
    F2:Fn(&mut ENV,&A) + 'static;
}

pub trait CellSink<ENV,A>: Cell<ENV,A> {
    fn change_value<F>(&self, env: &mut ENV, with_frp_context: &F, value: A)
    where F:WithFrpContext<ENV>;
}

#[derive(Copy,Clone)]
struct CellRef<ENV,A> {
    id: u32,
    env_phantom: PhantomData<ENV>,
    value_phantom: PhantomData<A>
}

impl<ENV,A> CellRef<ENV,A> {
    fn of(id: u32) -> CellRef<ENV,A> {
        CellRef {
            id: id,
            env_phantom: PhantomData,
            value_phantom: PhantomData
        }
    }
}

impl<ENV,A:'static> CellRef<ENV,A> {
    fn current_value<'a,F>(self, env: &'a mut ENV, with_frp_context: &F) -> &'a A
    where
    F:WithFrpContext<ENV>
    {
        let mut value_op: Option<*const A> = None;
        with_frp_context.with_frp_context(
            env,
            move |frp_context| {
                match frp_context.cell_map.get(&self.id) {
                    Some(cell) => {
                        match cell.value.as_ref().downcast_ref::<A>() {
                            Some(value) => {
                                value_op = Some(value);
                            },
                            None => ()
                        }
                    },
                    None => ()
                }
            }
        );
        match value_op {
            Some(value) => {
                unsafe { &(*value) }
            },
            None => panic!("")
        }
    }

    fn observe<F,F2>(self, env: &mut ENV, with_frp_context: &F, observer: F2) -> Box<FnOnce(&mut ENV, &F)>
    where
    F:WithFrpContext<ENV>,
    F2:Fn(&mut ENV,&A) + 'static
    {
        let mut observer_id_op: Option<u32> = None;
        let observer_id_op2: *mut Option<u32> = &mut observer_id_op;
        let cell_id = self.id.clone();
        with_frp_context.with_frp_context(
            env,
            move |frp_context| {
                if let Some(cell) = frp_context.cell_map.get_mut(&cell_id) {
                    let observer_id = cell.free_observer_id;
                    unsafe { *observer_id_op2 = Some(observer_id); }
                    cell.free_observer_id = cell.free_observer_id + 1;
                    cell.observer_map.insert(observer_id, Box::new(
                        move |env, value| {
                            match value.as_ref().downcast_ref::<A>() {
                                Some(value) => observer(env, value),
                                None => ()
                            }
                        }
                    ));
                }
            }
        );
        match observer_id_op {
            Some(observer_id) => {
                let cell_id = self.id.clone();
                return Box::new(move |env, with_frp_context| {
                    with_frp_context.with_frp_context(
                        env,
                        move |frp_context| {
                            if let Some(cell) = frp_context.cell_map.get_mut(&cell_id) {
                                cell.observer_map.remove(&observer_id);
                            }
                        }
                    );
                });
            },
            None => Box::new(|env, with_frp_context| {})
        }
    }
}

struct CellImpl<ENV,A> {
    id: u32,
    free_observer_id: u32,
    observer_map: HashMap<u32,Box<Fn(&mut ENV,&A)>>,
    update_fn: Box<Fn(&FrpContext<ENV>)->A>,
    dependent_cells: Vec<u32>,
    value: A
}

impl<ENV,A:'static> Cell<ENV,A> for CellImpl<ENV,A> {

    fn current_value<'a>(&'a self) -> &'a A {
        return &self.value;
    }

    fn observe<F,F2>(&self, env: &mut ENV, with_frp_context: &F, observer: F2) -> Box<Fn(&mut Self)>
    where
    F:WithFrpContext<ENV>,
    F2:Fn(&mut ENV,&A) + 'static
    {
        let observer_id = self.free_observer_id;
        with_frp_context.with_frp_context(
            env,
            move |frp_context| {
                if let Some(cell) = frp_context.cell_map.get_mut(&observer_id) {
                    cell.free_observer_id = cell.free_observer_id + 1;
                    cell.observer_map.insert(observer_id, Box::new(
                        move |env, value| {
                            match value.as_ref().downcast_ref::<A>() {
                                Some(value) => observer(env, value),
                                None => ()
                            }
                        }
                    ));
                }
            }
        );
        Box::new(move |cell| {
            cell.observer_map.remove(&observer_id);
        })
    }
}

impl<ENV:'static,A:'static + Clone> CellSink<ENV,A> for CellImpl<ENV,A> {
    fn change_value<F>(&self, env: &mut ENV, with_frp_context: &F, value: A)
    where F:WithFrpContext<ENV> {
        let cell_id = self.id.clone();
        let mut dependent_cells = Vec::new();
        for dependent_cell in &self.dependent_cells {
            dependent_cells.push(dependent_cell.clone());
        }
        FrpContext::transaction(
            env,
            with_frp_context,
            move |env, with_frp_context| {
                with_frp_context.with_frp_context(
                    env,
                    move |frp_context| {
                        if let Some(cell) = frp_context.cell_map.get_mut(&cell_id) {
                            cell.value = Box::new(value.clone()) as Box<Any>;
                        }
                        frp_context.mark_all_decendent_cells_for_update(cell_id, &mut HashSet::new());
                    }
                );
            }
        );
    }
}
