use sodium::HandlerRefMut;
use sodium::HasNode;
use sodium::SodiumCtx;
use sodium::StreamWithSend;
use sodium::Transaction;
use sodium::TransactionHandlerRef;
use std::cell::RefCell;
use std::rc::Rc;

pub struct CoalesceHandler<A> {
    data: Rc<RefCell<CoalesceHandlerData<A>>>
}

impl<A> Clone for CoalesceHandler<A> {
    fn clone(&self) -> Self {
        CoalesceHandler {
            data: self.data.clone()
        }
    }
}

pub struct CoalesceHandlerData<A> {
    f: Rc<Fn(&A,&A)->A>,
    out: StreamWithSend<A>,
    accum_op: Option<A>
}

impl<A: 'static + Clone> CoalesceHandler<A> {
    pub fn new<F>(f: F, out: &StreamWithSend<A>) -> CoalesceHandler<A> where F: Fn(&A,&A)->A + 'static {
        CoalesceHandler {
            data: Rc::new(RefCell::new(CoalesceHandlerData {
                f: Rc::new(f),
                out: out.clone(),
                accum_op: None
            }))
        }
    }

    pub fn to_transaction_handler(&self) -> TransactionHandlerRef<A> {
        let self_ = self.clone();
        TransactionHandlerRef::new(
            move |sodium_ctx: &mut SodiumCtx, trans: &mut Transaction, a: &A| {
                self_.run(sodium_ctx, trans, a);
            }
        )
    }

    pub fn run(&self, sodium_ctx: &mut SodiumCtx, trans1: &mut Transaction, a: &A) {
        let self_ = self.clone();
        let mut data = self.data.borrow_mut();
        let data = &mut data;
        let accum_op_was_none = data.accum_op.is_none();
        data.accum_op = match &data.accum_op {
            &Some(ref accum) => {
                Some((data.f)(accum, a))
            }
            &None => {
                Some(a.clone())
            }
        };
        if accum_op_was_none {
            trans1.prioritized(
                sodium_ctx,
                data.out.stream.data.clone() as Rc<RefCell<HasNode>>,
                HandlerRefMut::new(
                    move |sodium_ctx: &mut SodiumCtx, trans2: &mut Transaction| {
                        let mut data = self_.data.borrow_mut();
                        match &data.accum_op {
                            &Some(ref accum) => {
                                data.out.send(
                                    sodium_ctx,
                                    trans2,
                                    accum
                                )
                            },
                            &None => ()
                        }
                        data.accum_op = None;
                    }
                )
            );
        }

    }
}
